use argparse::{ArgumentParser, List, StoreOption, StoreTrue};
use glob::glob;
use rayon::prelude::*;
use sha256::try_digest;
use std::{
    error::Error,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::exit,
    result::Result,
};

struct GlobalOptions {
    verbose: bool,
    force_rehash: bool,
    force_rename: bool,
    dry_run: bool,
    version: bool,
    output_dir: Option<String>,
    copy: bool,
    files: Vec<String>,
}

fn main() {
    let mut opts = GlobalOptions {
        verbose: false,
        force_rehash: false,
        force_rename: false,
        dry_run: false,
        version: false,
        copy: false,
        output_dir: None,
        files: vec![],
    };
    parse_args(&mut opts);

    if opts.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        exit(0);
    }

    filter_files(&opts.files)
        .par_iter()
        .for_each(|x| match process_file(&opts, &x) {
            Ok(result) => {
                if result.len() > 0 {
                    println!("\"{}\" -> {:?}", x, result);
                }
            }
            Err(msg) => {
                if opts.verbose {
                    eprintln!("Skipped \"{}\": {}", x, msg.to_string());
                }
            }
        });
}

fn parse_args(opts: &mut GlobalOptions) {
    let mut ap = ArgumentParser::new();
    ap.set_description("Rename files to their hash");
    ap.refer(&mut opts.dry_run).add_option(
        &["-d", "--dry-run"],
        StoreTrue,
        "Do not actually rename files",
    );
    ap.refer(&mut opts.force_rehash).add_option(
        &["-f", "--force-rehash"],
        StoreTrue,
        "Process the file even if it looks like it has already been processed",
    );
    ap.refer(&mut opts.force_rename).add_option(
        &["-F", "--force-rename"],
        StoreTrue,
        "Rename file even there is another file with the same result name",
    );
    ap.refer(&mut opts.output_dir).add_option(
        &["-o", "--output-dir"],
        StoreOption,
        "Renamed files are moved to this directory",
    );
    ap.refer(&mut opts.copy).add_option(
        &["-c", "--copy"],
        StoreTrue,
        "Copy files to new name instead of moving",
    );
    ap.refer(&mut opts.verbose).add_option(
        &["-v", "--verbose"],
        StoreTrue,
        "Print more information during processing",
    );
    ap.refer(&mut opts.version).add_option(
        &["-V", "--version"],
        StoreTrue,
        "Print version and exit",
    );
    ap.refer(&mut opts.files)
        .add_argument("file", List, "Files to process");
    ap.parse_args_or_exit();
}

fn filter_files(file_paths: &Vec<String>) -> Vec<String> {
    let mut valid_files = Vec::new();

    for path in file_paths {
        if fs::metadata(path).is_ok() {
            valid_files.push(path.clone());
        } else {
            // Try to match it as a glob
            let entries = glob(path).expect("Failed to read glob pattern");
            for entry in entries {
                match entry {
                    Ok(path) => {
                        if fs::metadata(&path).is_ok() {
                            valid_files.push(path.to_string_lossy().into_owned());
                        }
                    }
                    Err(e) => println!("{:?}", e),
                }
            }
        }
    }

    valid_files
}

fn process_file(opts: &GlobalOptions, raw_filename: &String) -> Result<String, Box<dyn Error>> {
    let path_file = Path::new(raw_filename);
    if fs::symlink_metadata(raw_filename)?.file_type().is_symlink()
        || !fs::metadata(raw_filename)?.file_type().is_file()
    {
        return Err(Box::from("Not a file"));
    }

    let file_name = get_str_from_osstr(&path_file.file_stem())?;

    if !opts.force_rehash && is_already_processed(&file_name) {
        return Err(Box::from("Already processed"));
    }

    let file_ext = get_str_from_osstr(&path_file.extension())?;
    let result_hash = try_digest(path_file)?;
    let result_filename = match file_ext.len() {
        0 => result_hash,
        _ => format!("{}.{}", result_hash, file_ext),
    };

    let path = match &opts.output_dir {
        Some(output_dir) => {
            let mut buf = PathBuf::from(output_dir);
            buf.push(result_filename.clone());
            buf
        },
        None => {
            let mut buf = PathBuf::from(path_file);
            buf.set_file_name(result_filename.clone());
            buf
        },
    };

    let result_path = match path.into_os_string().into_string() {
        Ok(s) => s,
        Err(_) => return Err(Box::from("Could not process filename")),
    };

    if !opts.force_rename && is_already_exists(&result_path) {
        return Err(Box::from("Already exists"));
    }

    if !opts.dry_run {
        if opts.copy {
            fs::copy(raw_filename, result_path.clone())?;
        } else {
            fs::rename(raw_filename, result_path.clone())?;
        } 
    }

    Ok(result_path)
}

fn get_str_from_osstr(osstr: &Option<&OsStr>) -> Result<String, Box<dyn Error>> {
    Ok(match osstr {
        Some(n) => match n.to_str() {
            Some(s) => s.to_string(),
            None => return Err(Box::from("Could not get string from OsStr")),
        },
        None => return Err(Box::from("Could not get string from OsStr")),
    })
}

fn is_already_processed(filename: &String) -> bool {
    filename.len() == 64 && filename.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_already_exists(filename: &String) -> bool {
    Path::new(filename).exists()
}
