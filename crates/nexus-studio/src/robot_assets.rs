//! Capture the transitive local resources of robot meshes without directory assumptions.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub fn resolve(base: &Path, reference: &str) -> Result<PathBuf, String> {
    let decoded = decode_uri(reference)?;
    let reference = decoded
        .strip_prefix("file://localhost")
        .or_else(|| decoded.strip_prefix("file://"))
        .unwrap_or(&decoded);
    let relative = reference.strip_prefix("package://");
    let mut candidates = vec![base.join(reference)];
    if let Some(relative) = relative {
        for parent in base.ancestors().take(8) {
            candidates.push(parent.join(relative));
            if let Some((package, rest)) = relative.split_once('/') {
                if parent.file_name().is_some_and(|n| n == package) {
                    candidates.push(parent.join(rest));
                }
            }
        }
    }
    for path in candidates {
        if path.is_file() {
            return path.canonicalize().map_err(|e| e.to_string());
        }
    }
    // Standalone exported packages often keep package:// URIs after their
    // containing directory has been renamed. Accept a unique adjacent match.
    if let Some((_, rest)) = relative.and_then(|s| s.split_once('/')) {
        let mut found = base
            .ancestors()
            .take(4)
            .map(|p| p.join(rest))
            .filter(|p| p.is_file())
            .filter_map(|p| p.canonicalize().ok())
            .collect::<Vec<_>>();
        found.sort();
        found.dedup();
        if found.len() == 1 {
            return Ok(found.remove(0));
        }
        if found.len() > 1 {
            return Err(format!("Ambiguous package asset: {reference}"));
        }
    }
    Err(format!(
        "Missing referenced asset: {reference} (from {})",
        base.display()
    ))
}
fn decode_uri(value: &str) -> Result<String, String> {
    let mut out = vec![];
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let raw = bytes.get(i + 1..i + 3).ok_or("Incomplete URI escape")?;
            let hex = std::str::from_utf8(raw).map_err(|_| "Invalid URI escape")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "Invalid URI escape")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "Asset URI is not UTF-8".into())
}

#[derive(Default)]
pub struct Graph {
    assets: BTreeMap<PathBuf, (String, Vec<u8>)>,
}
impl Graph {
    pub fn sources(&self) -> impl Iterator<Item = &PathBuf> {
        self.assets.keys()
    }
    pub fn len(&self) -> usize {
        self.assets.len()
    }
    pub fn capture(&mut self, source: &Path) -> Result<String, String> {
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        if let Some((name, _)) = self.assets.get(&source) {
            return Ok(name.clone());
        }
        if self.assets.len() >= 10000 {
            return Err("Robot has more than 10,000 referenced asset files".into());
        }
        let base_name = source
            .file_name()
            .ok_or("Asset has no filename")?
            .to_string_lossy();
        let safe_name = base_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let name = format!("{}-{safe_name}", self.assets.len());
        self.assets.insert(source.clone(), (name.clone(), vec![]));
        let bytes = fs::read(&source).map_err(|e| e.to_string())?;
        let bytes = match source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "obj" => self
                .obj(
                    &source,
                    &String::from_utf8(bytes).map_err(|_| "OBJ must be UTF-8")?,
                )?
                .into_bytes(),
            "mtl" => self
                .mtl(
                    &source,
                    &String::from_utf8(bytes).map_err(|_| "MTL must be UTF-8")?,
                )?
                .into_bytes(),
            "dae" => self
                .collada(
                    &source,
                    &String::from_utf8(bytes).map_err(|_| "Collada must be UTF-8")?,
                )?
                .into_bytes(),
            "gltf" => self.gltf(&source, &bytes)?,
            "glb" => self.glb(&source, &bytes)?,
            _ => bytes,
        };
        self.assets.get_mut(&source).unwrap().1 = bytes;
        Ok(name)
    }
    pub fn write(&self, directory: &Path) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        for (name, bytes) in self.assets.values() {
            fs::write(directory.join(name), bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn reference(&mut self, source: &Path, value: &str) -> Result<String, String> {
        let asset = resolve(
            source.parent().ok_or("Asset needs parent directory")?,
            value,
        )?;
        self.capture(&asset)
    }
    fn obj(&mut self, source: &Path, input: &str) -> Result<String, String> {
        let mut out = String::new();
        for line in input.lines() {
            if let Some(rest) = line
                .trim_start()
                .strip_prefix("mtllib")
                .filter(|s| s.starts_with(char::is_whitespace))
            {
                let tokens = shell_words::split(rest).map_err(|e| e.to_string())?;
                if tokens.is_empty() {
                    return Err("Empty OBJ material reference".into());
                }
                // Common exporters leave a single filename containing spaces unquoted.
                let refs = if resolve(source.parent().unwrap(), rest.trim()).is_ok() {
                    vec![rest.trim().to_string()]
                } else {
                    tokens
                };
                let mut names = vec![];
                for r in refs {
                    names.push(self.reference(source, &r)?);
                }
                out.push_str(&format!("mtllib {}\n", names.join(" ")));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        Ok(out)
    }
    fn mtl(&mut self, source: &Path, input: &str) -> Result<String, String> {
        let mut out = String::new();
        for line in input.lines() {
            let words = shell_words::split(line).map_err(|e| e.to_string())?;
            let keyword = words.first().map(String::as_str).unwrap_or("");
            if keyword.starts_with("map_")
                || ["bump", "disp", "decal", "refl", "norm"].contains(&keyword)
            {
                let mut index = 1;
                while words.get(index).is_some_and(|s| s.starts_with('-')) {
                    let flag = words[index].as_str();
                    index += 1;
                    match flag {
                        "-o" | "-s" | "-t" => {
                            let start = index;
                            while index < start+3 && words.get(index).is_some_and(|s| s.parse::<f64>().is_ok()) { index += 1; }
                            if index == start { return Err(format!("Missing values for MTL option {flag}")); }
                        }
                        "-mm" => index += 2,
                        "-blendu" | "-blendv" | "-boost" | "-bm" | "-cc" | "-clamp" | "-imfchan" | "-texres" | "-type" | "-colorspace" => index += 1,
                        _ => return Err(format!("Unsupported MTL texture option {flag}; cannot capture its filename reliably")),
                    }
                }
                if index >= words.len() {
                    return Err("MTL texture filename is missing".into());
                }
                let reference = words[index..].join(" ");
                let name = self.reference(source, &reference)?;
                out.push_str(&format!("{} {name}\n", words[..index].join(" ")));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        Ok(out)
    }
    fn collada(&mut self, source: &Path, input: &str) -> Result<String, String> {
        let doc = roxmltree::Document::parse(input).map_err(|e| format!("Invalid Collada: {e}"))?;
        let mut edits = vec![];
        for node in doc.descendants().filter(|n| n.is_text()) {
            if !node.ancestors().any(|n| n.has_tag_name("library_images"))
                || !node.ancestors().any(|n| n.has_tag_name("init_from"))
            {
                continue;
            }
            let value = node.text().unwrap_or("").trim();
            if value.is_empty() || value.starts_with("data:") || value.starts_with('#') {
                continue;
            }
            edits.push((node.range(), self.reference(source, value)?));
        }
        // External instance URLs can themselves refer to another Collada file.
        for node in doc.descendants().filter(|n| n.is_element()) {
            for attr in node
                .attributes()
                .filter(|a| matches!(a.name(), "url" | "source"))
            {
                let value = attr.value();
                if value.starts_with('#') || value.is_empty() {
                    continue;
                }
                if let Some((file, fragment)) = value.split_once('#') {
                    let target = format!("{}#{fragment}", self.reference(source, file)?);
                    edits.push((attr.range_value(), target));
                } else {
                    edits.push((attr.range_value(), self.reference(source, value)?));
                }
            }
        }
        let mut output = input.to_string();
        edits.sort_by_key(|(r, _)| r.start);
        for (range, replacement) in edits.into_iter().rev() {
            output.replace_range(range, &replacement);
        }
        Ok(output)
    }
    fn gltf(&mut self, source: &Path, input: &[u8]) -> Result<Vec<u8>, String> {
        let mut value: serde_json::Value =
            serde_json::from_slice(input).map_err(|e| e.to_string())?;
        for field in ["buffers", "images"] {
            if let Some(items) = value
                .get_mut(field)
                .and_then(serde_json::Value::as_array_mut)
            {
                for item in items {
                    if let Some(uri) = item["uri"].as_str().filter(|s| !s.starts_with("data:")) {
                        item["uri"] = self.reference(source, uri)?.into();
                    }
                }
            }
        }
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())
    }
    fn glb(&mut self, source: &Path, input: &[u8]) -> Result<Vec<u8>, String> {
        fn number(bytes: &[u8], offset: usize) -> Result<u32, String> {
            Ok(u32::from_le_bytes(
                bytes
                    .get(offset..offset + 4)
                    .ok_or("Truncated GLB")?
                    .try_into()
                    .unwrap(),
            ))
        }
        if input.get(..4) != Some(b"glTF")
            || number(input, 4)? != 2
            || number(input, 8)? as usize != input.len()
        {
            return Err("Invalid GLB 2 container".into());
        }
        let mut out = input[..12].to_vec();
        let mut offset = 12;
        let mut json = false;
        while offset < input.len() {
            let length = number(input, offset)? as usize;
            let kind = number(input, offset + 4)?;
            let bytes = input
                .get(offset + 8..offset + 8 + length)
                .ok_or("Truncated GLB chunk")?;
            let mut captured = if kind == 0x4e4f534a {
                if json {
                    return Err("Duplicate GLB JSON chunk".into());
                }
                json = true;
                self.gltf(source, bytes)?
            } else {
                bytes.to_vec()
            };
            while captured.len() % 4 != 0 {
                captured.push(if kind == 0x4e4f534a { b' ' } else { 0 });
            }
            out.extend_from_slice(&(captured.len() as u32).to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend(captured);
            offset += 8 + length;
        }
        if !json {
            return Err("GLB has no JSON chunk".into());
        }
        let len = u32::try_from(out.len()).map_err(|_| "GLB is too large")?;
        out[8..12].copy_from_slice(&len.to_le_bytes());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captures_collada_texture_uris_and_gltf_buffers() {
        let root = std::env::temp_dir().join(format!(
            "nexus-asset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("pkg/meshes")).unwrap();
        fs::create_dir_all(root.join("pkg/textures")).unwrap();
        fs::write(root.join("pkg/textures/red paint.png"), b"texture").unwrap();
        fs::write(root.join("pkg/meshes/model.dae"),r#"<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema"><library_images><image id="red"><init_from>../textures/red%20paint.png</init_from></image></library_images></COLLADA>"#).unwrap();
        fs::write(root.join("pkg/meshes/data.bin"), b"buffer").unwrap();
        fs::write(root.join("pkg/meshes/model.gltf"),r#"{"buffers":[{"uri":"data.bin"}],"images":[{"uri":"../textures/red%20paint.png"},{"uri":"data:image/png;base64,aA=="}]}"#).unwrap();
        let resolved = resolve(&root.join("pkg/meshes"), "package://pkg/meshes/model.dae").unwrap();
        let mut graph = Graph::default();
        let dae = graph.capture(&resolved).unwrap();
        let gltf = graph.capture(&root.join("pkg/meshes/model.gltf")).unwrap();
        assert_eq!(graph.len(), 4);
        graph.write(&root.join("captured")).unwrap();
        fs::remove_dir_all(root.join("pkg")).unwrap();
        let mut restored = Graph::default();
        restored.capture(&root.join("captured").join(dae)).unwrap();
        restored.capture(&root.join("captured").join(gltf)).unwrap();
        assert_eq!(restored.len(), 4);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_nested_material_texture_is_an_import_error() {
        let root =
            std::env::temp_dir().join(format!("nexus-missing-texture-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("model.obj"), "mtllib paint.mtl\n").unwrap();
        fs::write(root.join("paint.mtl"), "newmtl red\nmap_Kd missing.png\n").unwrap();
        assert!(Graph::default()
            .capture(&root.join("model.obj"))
            .unwrap_err()
            .contains("missing.png"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn glb_captures_external_images_and_retains_embedded_binary_chunk() {
        let root = std::env::temp_dir().join(format!("nexus-glb-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("texture.png"), b"image").unwrap();
        let mut json = br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4}],"images":[{"uri":"texture.png"}]}"#.to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut input = b"glTF".to_vec();
        input.extend(2u32.to_le_bytes());
        input.extend(((12 + 8 + json.len() + 8 + 4) as u32).to_le_bytes());
        input.extend((json.len() as u32).to_le_bytes());
        input.extend(0x4e4f534au32.to_le_bytes());
        input.extend(json);
        input.extend(4u32.to_le_bytes());
        input.extend(0x004e4942u32.to_le_bytes());
        input.extend([1, 2, 3, 4]);
        fs::write(root.join("model.glb"), input).unwrap();
        let mut graph = Graph::default();
        let name = graph.capture(&root.join("model.glb")).unwrap();
        graph.write(&root.join("captured")).unwrap();
        fs::remove_file(root.join("texture.png")).unwrap();
        fs::remove_file(root.join("model.glb")).unwrap();
        let output = fs::read(root.join("captured").join(&name)).unwrap();
        assert_eq!(&output[output.len() - 4..], &[1, 2, 3, 4]);
        assert_eq!(
            u32::from_le_bytes(output[8..12].try_into().unwrap()) as usize,
            output.len()
        );
        let mut restored = Graph::default();
        restored.capture(&root.join("captured").join(name)).unwrap();
        assert_eq!(restored.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
