use anyhow::*;
use glob::glob;
use std::{collections::HashMap, path::PathBuf};
use std::{
    fs::{read_to_string, write, File},
    path::Path,
};
use texture_atlas::*;
use texture_packer::{
    exporter::ImageExporter, importer::ImageImporter, texture::Texture, TexturePacker,
    TexturePackerConfig,
};

struct ShaderData {
    src: String,
    src_path: PathBuf,
    spv_path: PathBuf,
    kind: shaderc::ShaderKind,
}

impl ShaderData {
    pub fn load(src_path: PathBuf) -> Result<Self> {
        let extension = src_path
            .extension()
            .context("File has no extension")?
            .to_str()
            .context("Extension cannot be converted to &str")?;
        let kind = match extension {
            "vert" => shaderc::ShaderKind::Vertex,
            "frag" => shaderc::ShaderKind::Fragment,
            "comp" => shaderc::ShaderKind::Compute,
            _ => bail!("Unsupported shader: {}", src_path.display()),
        };

        let src = read_to_string(src_path.clone())?;
        let spv_path = src_path.with_extension(format!("{}.spv", extension));

        Ok(Self {
            src,
            src_path,
            spv_path,
            kind,
        })
    }
}

fn compile_shaders() -> Result<()> {
    // This tells cargo to rerun this script if something in /src/graphics changes.
    println!("cargo:rerun-if-changed=src/graphics/*");

    // Collect all shaders recursively within /src/
    let mut shader_paths = [
        glob("./src/graphics/**/*.vert")?,
        glob("./src/graphics/**/*.frag")?,
        glob("./src/graphics/**/*.comp")?,
    ];

    // This could be parallelized
    let shaders = shader_paths
        .iter_mut()
        .flatten()
        .map(|glob_result| ShaderData::load(glob_result?))
        .collect::<Vec<Result<_>>>()
        .into_iter()
        .collect::<Result<Vec<_>>>();

    let mut compiler = shaderc::Compiler::new().context("Unable to create shader compiler")?;

    // This can't be parallelized. The [shaderc::Compiler] is not
    // thread safe. Also, it creates a lot of resources. You could
    // spawn multiple processes to handle this, but it would probably
    // be better just to only compile shaders that have been changed
    // recently.
    for shader in shaders? {
        let compiled = compiler.compile_into_spirv(
            &shader.src,
            shader.kind,
            &shader.src_path.to_str().unwrap(),
            "main",
            None,
        )?;
        write(shader.spv_path, compiled.as_binary_u8())?;
    }

    Ok(())
}

fn render_filename(path: &Path) -> String {
    path.components()
        .map(|component| match component {
            std::path::Component::Prefix(_) => {
                panic!("prefix not supported");
            }
            std::path::Component::RootDir => {
                panic!("root dir not supported");
            }
            std::path::Component::CurDir => {
                panic!("current dir not supported");
            }
            std::path::Component::ParentDir => {
                panic!("parent dir not supported");
            }
            std::path::Component::Normal(name) => name.to_str().unwrap().to_string(),
        })
        .fold(
            String::new(),
            |l, r| {
                if l.is_empty() {
                    r
                } else {
                    l + "/" + &r
                }
            },
        )
}

fn pack_textures() -> Result<()> {
    let config = TexturePackerConfig {
        max_width: 512,
        max_height: 512,
        allow_rotation: false,
        border_padding: 2,
        ..Default::default()
    };

    let mut packer = TexturePacker::new_skyline(config);

    for path in glob("./assets_raw/ui/*.png")?.into_iter() {
        let path = path?;
        let texture = ImageImporter::import_from_file(&path).unwrap();
        let canonical_path = path.canonicalize()?;
        let name = canonical_path.strip_prefix(Path::new("./assets_raw/ui/").canonicalize()?)?;
        packer.pack_own(render_filename(name), texture).unwrap();
    }

    // Print the information
    // println!("Dimensions : {}x{}", packer.width(), packer.height());
    // for (name, frame) in packer.get_frames() {
    //     println!("  {:7} : {:?}", name, frame.frame);
    // }

    // Save the packed image.
    let exporter = ImageExporter::export(&packer).unwrap();
    let _ = std::fs::create_dir("./assets/ui"); // ignore whether the directory already existed
    let mut file = File::create("./assets/ui/atlas.png").unwrap();
    exporter
        .write_to(&mut file, image::ImageFormat::Png)
        .unwrap();

    // Save the atlas information.
    let mut frames = HashMap::new();
    for (name, frame) in packer.get_frames() {
        frames.insert(
            name.clone(),
            TextureRegionInformation {
                frame: Rect {
                    x: frame.frame.x,
                    y: frame.frame.y,
                    w: frame.frame.w,
                    h: frame.frame.h,
                },
                rotated: frame.rotated,
                trimmed: frame.trimmed,
                source: Rect {
                    x: frame.source.x,
                    y: frame.source.y,
                    w: frame.source.w,
                    h: frame.source.h,
                },
            },
        );
    }
    let atlas = TextureAtlas {
        width: packer.width(),
        height: packer.height(),
        frames,
    };
    let atlas_file = File::create("./assets/ui/atlas.json").unwrap();
    serde_json::to_writer(&atlas_file, &atlas)?;

    Ok(())
}

fn main() -> Result<()> {
    compile_shaders()?;
    pack_textures()?;

    Ok(())
}
