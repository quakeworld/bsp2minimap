use anyhow::Result;
use bspparser::helpers::{
    get_face_texture, get_face_vertice_indexes, get_face_vertices, read_texture_image, TextureScale,
};
use bspparser::{BspFile, Face};
use std::collections::HashMap;
use std::io::{Read, Seek};

pub enum ProjectionAxis {
    X,
    Y,
    Z,
}

pub struct StuffToDraw {
    pub points: Vec<(f32, f32)>,
    pub texture_name: String,
    pub min_z: f32,
    pub max_z: f32,
}

pub fn convert<R>(r: &mut R, filename: &str) -> Result<()>
where
    R: Read + Seek,
{
    let bsp = BspFile::parse(r)?;
    let axis = ProjectionAxis::Z;

    // 1. Projected vertices
    // Project the 3D vertices onto a 2D plane based on the chosen projection axis:
    // For z-axis projection (top-down view), use x and y coordinates.
    // For y-axis projection (side view), use x and z coordinates.
    // For x-axis projection (front view), use y and z coordinates.
    #[rustfmt::skip]
    let pvertices: Vec<(f32, f32)> = bsp.vertices.iter().map(|v| match axis {
        ProjectionAxis::X => (v.y, v.z),
        ProjectionAxis::Y => (v.x, v.z),
        ProjectionAxis::Z => (v.x, -v.y), // flip y
    }).collect();

    // get average color for each texture_name
    let mut color_per_tex_name: HashMap<String, (u32, u32, u32)> = HashMap::new();

    for tex in &bsp.textures {
        let im = read_texture_image(r, tex, TextureScale::Eighth)?;

        // im.data contains a byte array of all pixels
        // chunk to 3 and then calculate average color
        let mut r = 0.;
        let mut g = 0.;
        let mut b = 0.;
        for i in 0..im.data.len() / 3 {
            r += im.data[i * 3] as f32;
            g += im.data[i * 3 + 1] as f32;
            b += im.data[i * 3 + 2] as f32;
        }
        let total = im.data.len() as f32 / 3.0;
        r /= total;
        g /= total;
        b /= total;

        color_per_tex_name.insert(tex.name.to_string(), (r as u32, g as u32, b as u32));
    }

    // 2. Generate Polygons paths
    // Filter and sort faces based on the projection axis to ensure correct rendering order.
    // For each face, generate a polygon using the projected_vertices.
    let stuff_to_draw: Vec<StuffToDraw> = filter_and_sort_faces(&bsp, &axis)
        .iter()
        .map(|face| {
            let points = get_face_vertice_indexes(&bsp, face)
                .iter()
                .map(|vertex_index| pvertices[*vertex_index as usize])
                .collect::<Vec<(f32, f32)>>();
            let texture_name = get_face_texture(&bsp, face).name.to_string();
            StuffToDraw {
                points: points.clone(),
                texture_name,
                max_z: get_face_vertices(&bsp, face)
                    .iter()
                    .map(|v| v.z)
                    .reduce(f32::max)
                    .unwrap(),
                min_z: get_face_vertices(&bsp, face)
                    .iter()
                    .map(|v| v.z)
                    .reduce(f32::min)
                    .unwrap(),
            }
        })
        // skip empty
        .filter(|s| !s.points.is_empty())
        .collect();

    // 3. Generate SVG
    // add polygons and other necessary elements (e.g., background, borders).
    let padding = 100.0;

    let mut bsp_group = svg::node::element::Group::new().set("id", "bsp_ref");

    for item in stuff_to_draw.iter() {
        let points_str = item
            .points
            .iter()
            .map(|(x, y)| format!("{},{}", x, y))
            .collect::<Vec<String>>()
            .join(" ");

        let fill_color = color_per_tex_name
            .get(&item.texture_name)
            .unwrap_or(&(255, 255, 255));

        // convert to hex representation
        let fill_color = format!(
            "#{:02x}{:02x}{:02x}",
            fill_color.0, fill_color.1, fill_color.2
        );

        bsp_group = bsp_group.add(
            svg::node::element::Polygon::new()
                .set("points", points_str)
                .set("fill", fill_color),
        );
    }

    #[rustfmt::skip]
    let bounds = (
        pvertices.clone().into_iter().map(|(x, _)| x).reduce(f32::min).unwrap(),
        pvertices.clone().into_iter().map(|(x, _)| x).reduce(f32::max).unwrap(),
        pvertices.clone().into_iter().map(|(_, y)| y).reduce(f32::min).unwrap(),
        pvertices.clone().into_iter().map(|(_, y)| y).reduce(f32::max).unwrap(),
    );

    let viewbox = (
        bounds.0 - padding,
        bounds.2 - padding,
        bounds.1 - bounds.0 + 2. * padding,
        bounds.3 - bounds.2 + 2. * padding,
    );

    let mut doc = svg::Document::new()
        .set(
            "viewBox",
            format!("{} {} {} {}", viewbox.0, viewbox.1, viewbox.2, viewbox.3),
        )
        .add(
            // background
            svg::node::element::Rectangle::new()
                .set("x", viewbox.0)
                .set("y", viewbox.1)
                .set("width", viewbox.2)
                .set("height", viewbox.3)
                .set("fill", "black"),
        )
        .add(svg::node::element::Definitions::new().add(bsp_group));

    doc = doc.add(
        svg::node::element::Use::new()
            .set("href", "#bsp_ref")
            .set("stroke", "black")
            .set("stroke-width", 10)
            .set("stroke-miterlimit", 0),
    );
    doc = doc.add(
        svg::node::element::Use::new()
            .set("href", "#bsp_ref")
            .set("fill", "#eee")
            .set("stroke", "black")
            .set("stroke-width", "0.5"),
    );

    svg::save(format!("target/{filename}.svg"), &doc)?;

    let unique = stuff_to_draw
        .iter()
        .map(|t| format!("{}-{}: {}", t.min_z, t.max_z, t.texture_name))
        .collect::<std::collections::HashSet<String>>();
    dbg!(&filename);
    dbg!(&unique);

    Ok(())
}

pub fn filter_and_sort_faces(bsp: &BspFile, axis: &ProjectionAxis) -> Vec<Face> {
    let mut faces: Vec<Face> = filter_faces(bsp);

    let minimums: HashMap<usize, f32> = {
        let mut result = HashMap::new();
        for face in faces.iter() {
            let vertices = get_face_vertices(bsp, face);
            let min = match axis {
                ProjectionAxis::X => vertices.iter().map(|v| v.x).reduce(f32::min),
                ProjectionAxis::Y => vertices.iter().map(|v| v.y).reduce(f32::min),
                ProjectionAxis::Z => vertices.iter().map(|v| v.z).reduce(f32::min),
            }
            .unwrap();
            result.insert(face.edge_list_index as usize, min);
        }
        result
    };

    faces.sort_by(|a, b| {
        let a_min = minimums[&(a.edge_list_index as usize)];
        let b_min = minimums[&(b.edge_list_index as usize)];
        a_min.partial_cmp(&b_min).unwrap()
    });
    faces
}

pub fn filter_faces(bsp: &BspFile) -> Vec<Face> {
    bsp.faces
        .iter()
        .cloned()
        .filter(|face| {
            let texture_name = get_face_texture(bsp, face).name.to_string();
            !is_ignored_texture(&texture_name)
        })
        .collect()
}

pub fn is_ignored_texture(name: &str) -> bool {
    let ignored_names = ["clip", "hint", "trigger", "163"];

    if ignored_names.iter().any(|n| *n == name) {
        return true;
    }

    let ignored_needles = ["sky", "light", "tech", "wood"];
    ignored_needles.iter().any(|needle| name.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use image::save_buffer;
    use std::fs::File;

    #[test]
    fn test_convert() -> Result<()> {
        let mapnames = ["dm2", "dm3_gpl", "dm4", "e1m2", "schloss"];
        let mapnames = ["e1m2"];

        for name in mapnames.iter() {
            let file = &mut File::open(format!("tests/files/{}.bsp", name))?;
            let _res = convert(file, name);

            // create dir with same name as name
            /*std::fs::create_dir_all(format!("target/{}", name))?;
            file.seek(std::io::SeekFrom::Start(0))?;
            let bsp = BspFile::parse(file)?;

            for tex in &bsp.textures {
                let im = read_texture_image(file, tex, TextureScale::Full)?;
                let path = format!("target/{}/{}.png", name, tex.name);
                save_buffer(path, &im.data, im.width, im.height, image::ColorType::Rgb8)?;
            }*/
        }

        Ok(())
    }
}
