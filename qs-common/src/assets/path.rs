use std::path::PathBuf;
use tokio::io::BufReader;
use tokio::fs::File;

/// Represents the path to an asset on disk, stored relative to the `assets` directory.
#[derive(Eq, PartialEq, Clone, Hash)]
pub struct AssetPath {
    segments: Vec<String>,
}

use lazy_static::lazy_static;
lazy_static! {
    static ref ASSET_FOLDER: PathBuf = find_folder::Search::Kids(3).for_folder("assets").expect("Could not find asset dir");
}

impl AssetPath {
    /// Creates a path from a list of segments. Segments like `..` and `.` are supported.
    pub fn new(segments: Vec<String>) -> Self {
        let mut new_segments = Vec::new();

        for segment in segments {
            match segment.as_str() {
                "." => {}
                ".." => {
                    if new_segments.is_empty() {
                        panic!("Could not parse path, use of `..` would escape asset directory");
                    } else {
                        new_segments.pop();
                    }
                }
                _ => {
                    new_segments.push(segment);
                }
            }
        }

        AssetPath {
            segments: new_segments,
        }
    }

    pub fn to_path(&self) -> PathBuf {
        let mut path = ASSET_FOLDER.clone();
        for segment in &self.segments {
            path.push(segment);
        }
        path
    }

    pub async fn read_file(&self) -> std::io::Result<BufReader<File>> {
        let f = File::open(self.to_path()).await?;
        Ok(BufReader::new(f))
    }
}
