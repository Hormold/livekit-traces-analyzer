//! Input handling - supports folders, ZIP files, and PCAP files.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Input type detection result.
#[derive(Debug, Clone)]
pub enum InputType {
    /// A folder with logs.json and spans.json
    Folder(PathBuf),
    /// A ZIP file containing observability data
    ZipFile(PathBuf),
    /// A PCAP file with network capture
    PcapFile(PathBuf),
}

/// Detected and extracted input ready for analysis.
pub struct PreparedInput {
    /// Path to the folder with observability data (may be temp dir for ZIP)
    pub traces_folder: Option<PathBuf>,
    /// Path to PCAP file if provided
    pub pcap_file: Option<PathBuf>,
    /// Temp directory to clean up (if ZIP was extracted)
    pub _temp_dir: Option<tempfile::TempDir>,
}

/// Detect the type of input path.
pub fn detect_input_type(path: &Path) -> Result<InputType> {
    if !path.exists() {
        bail!("Path does not exist: {}", path.display());
    }

    if path.is_dir() {
        return Ok(InputType::Folder(path.to_path_buf()));
    }

    // Check file extension and magic bytes
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match extension.as_deref() {
        Some("zip") => Ok(InputType::ZipFile(path.to_path_buf())),
        Some("pcap") | Some("pcapng") => Ok(InputType::PcapFile(path.to_path_buf())),
        _ => {
            // Check magic bytes
            let mut file = File::open(path)?;
            let mut magic = [0u8; 4];
            file.read_exact(&mut magic)?;

            // ZIP magic: PK\x03\x04
            if magic == [0x50, 0x4B, 0x03, 0x04] {
                return Ok(InputType::ZipFile(path.to_path_buf()));
            }

            // PCAP magic: \xd4\xc3\xb2\xa1 (little-endian) or \xa1\xb2\xc3\xd4 (big-endian)
            if magic == [0xd4, 0xc3, 0xb2, 0xa1] || magic == [0xa1, 0xb2, 0xc3, 0xd4] {
                return Ok(InputType::PcapFile(path.to_path_buf()));
            }

            // PCAPNG magic: \x0a\x0d\x0d\x0a
            if magic == [0x0a, 0x0d, 0x0d, 0x0a] {
                return Ok(InputType::PcapFile(path.to_path_buf()));
            }

            bail!("Unknown file type: {}", path.display());
        }
    }
}

/// Extract a ZIP file to a temporary directory.
pub fn extract_zip(zip_path: &Path) -> Result<(PathBuf, tempfile::TempDir)> {
    let file = File::open(zip_path)
        .with_context(|| format!("Failed to open ZIP file: {}", zip_path.display()))?;

    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("Failed to read ZIP archive: {}", zip_path.display()))?;

    let temp_dir = tempfile::tempdir()
        .context("Failed to create temporary directory")?;

    // Find the root folder in the ZIP (often ZIPs have a single root folder)
    let mut root_folder: Option<String> = None;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();
        if name.contains('/') {
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() > 1 {
                let potential_root = parts[0].to_string();
                if root_folder.is_none() {
                    root_folder = Some(potential_root);
                }
            }
        }
    }

    // Extract all files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = temp_dir.path().join(file.name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    // Return the path to extracted data
    let extract_path = if let Some(root) = root_folder {
        temp_dir.path().join(root)
    } else {
        temp_dir.path().to_path_buf()
    };

    // Check if it looks like observability data
    let logs_path = extract_path.join("logs.json");
    let spans_path = extract_path.join("spans.json");

    if logs_path.exists() || spans_path.exists() {
        Ok((extract_path, temp_dir))
    } else {
        // Maybe files are directly in temp_dir
        let logs_direct = temp_dir.path().join("logs.json");
        let spans_direct = temp_dir.path().join("spans.json");

        if logs_direct.exists() || spans_direct.exists() {
            Ok((temp_dir.path().to_path_buf(), temp_dir))
        } else {
            // List what we found
            let contents: Vec<_> = fs::read_dir(temp_dir.path())?
                .filter_map(|e| e.ok())
                .map(|e| e.path().display().to_string())
                .take(10)
                .collect();

            bail!(
                "ZIP extracted but no logs.json/spans.json found. Contents: {:?}",
                contents
            );
        }
    }
}

/// Prepare input for analysis - handles folders, ZIPs, and PCAPs.
pub fn prepare_input(paths: &[PathBuf]) -> Result<PreparedInput> {
    let mut traces_folder: Option<PathBuf> = None;
    let mut pcap_file: Option<PathBuf> = None;
    let mut temp_dir: Option<tempfile::TempDir> = None;

    for path in paths {
        let input_type = detect_input_type(path)?;

        match input_type {
            InputType::Folder(p) => {
                if traces_folder.is_some() {
                    bail!("Multiple trace folders specified");
                }
                traces_folder = Some(p);
            }
            InputType::ZipFile(p) => {
                if traces_folder.is_some() {
                    bail!("Multiple trace sources specified (folder + ZIP)");
                }
                let (extracted, td) = extract_zip(&p)?;
                traces_folder = Some(extracted);
                temp_dir = Some(td);
            }
            InputType::PcapFile(p) => {
                if pcap_file.is_some() {
                    bail!("Multiple PCAP files specified");
                }
                pcap_file = Some(p);
            }
        }
    }

    if traces_folder.is_none() && pcap_file.is_none() {
        bail!("No valid input provided. Need a traces folder/ZIP and/or a PCAP file.");
    }

    Ok(PreparedInput {
        traces_folder,
        pcap_file,
        _temp_dir: temp_dir,
    })
}
