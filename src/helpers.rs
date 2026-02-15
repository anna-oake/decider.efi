use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use uefi::proto::media::file::{File, FileAttribute, FileHandle, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::{CString16, Status};

use crate::config::Choice;

pub fn normalize_uefi_path(path: &str) -> String {
    path.chars()
        .map(|c| if c == '/' { '\\' } else { c })
        .collect()
}

pub fn to_cstring16(s: &str) -> Result<CString16, Status> {
    CString16::try_from(s).map_err(|_| Status::INVALID_PARAMETER)
}

pub fn utf16_nul_terminated_bytes(value: &str) -> Vec<u8> {
    let mut code_units: Vec<u16> = value.encode_utf16().collect();
    code_units.push(0);

    let mut bytes = Vec::with_capacity(code_units.len() * 2);
    for code_unit in code_units {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

pub fn read_text_file(fs: &mut SimpleFileSystem, path: &str) -> Result<String, Status> {
    let bytes = read_binary_file(fs, path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn read_binary_file(fs: &mut SimpleFileSystem, path: &str) -> Result<Vec<u8>, Status> {
    let path = to_cstring16(path)?;
    let mut root = fs.open_volume().map_err(|e| e.status())?;
    let handle: FileHandle = root
        .open(path.as_ref(), FileMode::Read, FileAttribute::empty())
        .map_err(|e| e.status())?;

    let typed_handle = match handle.into_type() {
        Ok(typed_handle) => typed_handle,
        Err(err) => return Err(err.status()),
    };

    let mut file = match typed_handle {
        FileType::Regular(file) => file,
        _ => return Err(Status::UNSUPPORTED),
    };

    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let read = file.read(&mut chunk).map_err(|e| e.status())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    Ok(bytes)
}

pub fn parse_key_values(text: &str) -> Vec<(String, String)> {
    let mut kv = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), value.trim()),
            None => continue,
        };

        if key.is_empty() || value.is_empty() {
            continue;
        }

        kv.push((key.to_owned(), value.to_owned()));
    }

    kv
}

pub fn get_required_value<'a>(kv: &'a [(String, String)], key: &str) -> Result<&'a str, Status> {
    for (k, v) in kv {
        if k == key {
            return Ok(v.as_str());
        }
    }
    Err(Status::LOAD_ERROR)
}

pub fn get_optional_value<'a>(kv: &'a [(String, String)], key: &str) -> Option<&'a str> {
    for (k, v) in kv {
        if k == key {
            return Some(v.as_str());
        }
    }
    None
}

pub fn parse_choice(text: &str) -> Result<Choice, Status> {
    let kv = parse_key_values(text);
    let choice_type = get_required_value(&kv, "choice_type")?;

    match choice_type {
        "entry_id" => Ok(Choice::Entry(get_required_value(&kv, "entry_id")?.to_owned())),
        "nixos-current" => Ok(Choice::NixosCurrent),
        chainload_type if chainload_type.starts_with("chainload_") && chainload_type.len() > "chainload_".len() => {
            Ok(Choice::Chainload(chainload_type.to_owned()))
        }
        _ => Err(Status::LOAD_ERROR),
    }
}
