use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileHandle, FileMode, FileType};

use crate::helpers::to_cstring16;

const LOADER_ENTRIES_PATH: &str = "\\loader\\entries";
const EFI_LINUX_PATH: &str = "\\EFI\\Linux";

pub fn find_latest_nixos_entry(image_handle: Handle) -> Result<String, Status> {
    let mut fs = uefi::boot::get_image_file_system(image_handle).map_err(|e| e.status())?;
    let mut best: Option<(u64, String)> = None;

    consider_nixos_entries_in_dir(
        &mut fs,
        LOADER_ENTRIES_PATH,
        parse_nixos_generation_conf_name,
        &mut best,
    )?;
    consider_nixos_entries_in_dir(
        &mut fs,
        EFI_LINUX_PATH,
        parse_nixos_generation_efi_name,
        &mut best,
    )?;

    best.map(|(_, id)| id).ok_or(Status::NOT_FOUND)
}

fn consider_nixos_entries_in_dir(
    fs: &mut uefi::proto::media::fs::SimpleFileSystem,
    directory_path: &str,
    parse_entry: fn(&str) -> Option<(u64, String)>,
    best: &mut Option<(u64, String)>,
) -> Result<(), Status> {
    let path = to_cstring16(directory_path)?;
    let mut root = fs.open_volume().map_err(|e| e.status())?;
    let handle: FileHandle = match root.open(path.as_ref(), FileMode::Read, FileAttribute::empty()) {
        Ok(handle) => handle,
        Err(err) if err.status() == Status::NOT_FOUND => return Ok(()),
        Err(err) => return Err(err.status()),
    };

    let typed_handle = match handle.into_type() {
        Ok(typed_handle) => typed_handle,
        Err(err) => return Err(err.status()),
    };

    let mut dir = match typed_handle {
        FileType::Dir(dir) => dir,
        _ => return Ok(()),
    };

    loop {
        let Some(info) = dir.read_entry_boxed().map_err(|e| e.status())? else {
            break;
        };

        let name = format!("{}", info.file_name());
        if let Some((generation, entry_id)) = parse_entry(&name) {
            let is_newer = match best {
                Some((best_generation, _)) => generation > *best_generation,
                None => true,
            };
            if is_newer {
                *best = Some((generation, entry_id));
            }
        }
    }

    Ok(())
}

fn parse_nixos_generation_conf_name(name: &str) -> Option<(u64, String)> {
    const PREFIX: &str = "nixos-generation-";
    const SUFFIX: &str = ".conf";

    if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) {
        return None;
    }

    let number = &name[PREFIX.len()..name.len() - SUFFIX.len()];
    if number.is_empty() {
        return None;
    }

    let generation = number.parse::<u64>().ok()?;
    let entry_id = name[..name.len() - SUFFIX.len()].to_owned();
    Some((generation, entry_id))
}

fn parse_nixos_generation_efi_name(name: &str) -> Option<(u64, String)> {
    const PREFIX: &str = "nixos-generation-";
    const SUFFIX: &str = ".efi";

    if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) {
        return None;
    }

    let middle = &name[PREFIX.len()..name.len() - SUFFIX.len()];
    if middle.is_empty() {
        return None;
    }

    let number = middle.split_once('-').map_or(middle, |(prefix, _)| prefix);
    if number.is_empty() {
        return None;
    }

    let generation = number.parse::<u64>().ok()?;
    let entry_id = name[..name.len() - SUFFIX.len()].to_owned();
    Some((generation, entry_id))
}
