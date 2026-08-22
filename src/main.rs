use argparse::{ArgumentParser, List, StoreOption, StoreTrue};
use glob::glob;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
};

#[derive(Default)]
struct GlobalOptions {
    verbose: bool,
    force_rehash: bool,
    force_rename: bool,
    dry_run: bool,
    version: bool,
    extensions: Option<String>,
    output_dir: Option<PathBuf>,
    copy: bool,
    files: Vec<String>,
}

#[derive(Debug)]
enum ProcessFileError {
    AlreadyExists,
    AlreadyProcessed,
    InvalidPathComponent(&'static str),
    Io(io::Error),
    NotAFile,
    Hash(Box<dyn Error>),
}

impl fmt::Display for ProcessFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists => write!(f, "Already exists"),
            Self::AlreadyProcessed => write!(f, "Already processed"),
            Self::InvalidPathComponent(component) => {
                write!(f, "Could not read {component} from path")
            }
            Self::Io(err) => err.fmt(f),
            Self::NotAFile => write!(f, "Not a file"),
            Self::Hash(err) => err.fmt(f),
        }
    }
}

impl Error for ProcessFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Hash(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for ProcessFileError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<Box<dyn Error>> for ProcessFileError {
    fn from(err: Box<dyn Error>) -> Self {
        Self::Hash(err)
    }
}

fn main() {
    let mut opts = GlobalOptions::default();
    parse_args(&mut opts);

    if opts.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let extensions = parse_extensions(opts.extensions.as_deref());
    filter_files(&opts.files, extensions.as_deref(), opts.verbose)
        .par_iter()
        .for_each(|path| match process_file(&opts, path) {
            Ok(result) => println!("\"{}\" -> \"{}\"", path.display(), result.display()),
            Err(err) if opts.verbose => {
                eprintln!("Skipped \"{}\": {}", path.display(), err);
            }
            Err(_) => {}
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
    ap.refer(&mut opts.extensions).add_option(
        &["--extensions"],
        StoreOption,
        "Only process files with an extension in this comma-separated list",
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

fn filter_files(
    file_paths: &[String],
    extensions: Option<&[String]>,
    verbose: bool,
) -> Vec<PathBuf> {
    let mut valid_files = Vec::new();

    for path in file_paths {
        let candidate = Path::new(path);
        if candidate.exists() {
            if matches_extensions(candidate, extensions) {
                valid_files.push(candidate.to_path_buf());
            }
        } else {
            match glob(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(path) if path.exists() && matches_extensions(&path, extensions) => {
                                valid_files.push(path)
                            }
                            Ok(_) => {}
                            Err(err) if verbose => {
                                eprintln!("Skipped glob entry for \"{path}\": {err}")
                            }
                            Err(_) => {}
                        }
                    }
                }
                Err(err) if verbose => eprintln!("Invalid glob pattern \"{path}\": {err}"),
                Err(_) => {}
            }
        }
    }

    valid_files
}

fn parse_extensions(extensions: Option<&str>) -> Option<Vec<String>> {
    extensions.map(|extensions| {
        extensions
            .split(',')
            .map(str::trim)
            .map(|extension| extension.trim_start_matches('.'))
            .filter(|extension| !extension.is_empty())
            .map(normalize_extension)
            .collect()
    })
}

fn matches_extensions(path: &Path, extensions: Option<&[String]>) -> bool {
    let Some(extensions) = extensions else {
        return true;
    };
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    let extension = normalize_extension(extension);

    extensions.iter().any(|candidate| candidate == &extension)
}

fn process_file(opts: &GlobalOptions, raw_path: &Path) -> Result<PathBuf, ProcessFileError> {
    let metadata = fs::symlink_metadata(raw_path)?;
    if !metadata.file_type().is_file() {
        return Err(ProcessFileError::NotAFile);
    }

    let file_stem = path_component_to_str(raw_path.file_stem(), "file stem")?;
    if !opts.force_rehash && is_already_processed(file_stem) {
        return Err(ProcessFileError::AlreadyProcessed);
    }

    let result_hash = hash_file(raw_path)?;
    let result_filename = match raw_path.extension().and_then(OsStr::to_str) {
        Some(extension) if !extension.is_empty() => {
            format!("{result_hash}.{}", normalize_extension(extension))
        }
        _ => result_hash,
    };

    let result_path = match &opts.output_dir {
        Some(output_dir) => output_dir.join(&result_filename),
        None => raw_path.with_file_name(&result_filename),
    };

    if !opts.force_rename && result_path.exists() {
        return Err(ProcessFileError::AlreadyExists);
    }

    if !opts.dry_run {
        if opts.copy {
            fs::copy(raw_path, &result_path)?;
        } else {
            fs::rename(raw_path, &result_path)?;
        }
    }

    Ok(result_path)
}

fn path_component_to_str<'a>(
    component: Option<&'a OsStr>,
    name: &'static str,
) -> Result<&'a str, ProcessFileError> {
    component
        .and_then(OsStr::to_str)
        .ok_or(ProcessFileError::InvalidPathComponent(name))
}

fn is_already_processed(filename: &str) -> bool {
    filename.len() == 64 && filename.chars().all(|c| c.is_ascii_hexdigit())
}

fn normalize_extension(extension: &str) -> String {
    match extension.to_ascii_lowercase().as_str() {
        "jpeg" | "jpe" => "jpg".to_owned(),
        extension => extension.to_owned(),
    }
}

fn hash_file(path: &Path) -> Result<String, ProcessFileError> {
    let file = fs::File::open(path)?;
    let mut reader = io::BufReader::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();

    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        hasher.update(buf);
        let len = buf.len();
        reader.consume(len);
    }

    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }

    Ok(output)
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!("nibble must be in 0..=15"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs, process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!("hashname-test-{}-{id}", process::id()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn identifies_already_processed_hashes() {
        assert!(is_already_processed(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_already_processed("short"));
        assert!(!is_already_processed(
            "g123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
    }

    #[test]
    fn normalizes_extensions() {
        assert_eq!(normalize_extension("PNG"), "png");
        assert_eq!(normalize_extension("JPEG"), "jpg");
        assert_eq!(normalize_extension("jpe"), "jpg");
    }

    #[test]
    fn parses_case_insensitive_extension_lists() {
        assert_eq!(
            parse_extensions(Some(" JPG, .PnG, jpeg ")),
            Some(vec!["jpg".to_owned(), "png".to_owned(), "jpg".to_owned()])
        );
        assert_eq!(parse_extensions(None), None);
    }

    #[test]
    fn matches_only_requested_extensions() {
        let extensions = parse_extensions(Some("jpg,png")).unwrap();

        assert!(matches_extensions(
            Path::new("photo.JPEG"),
            Some(&extensions)
        ));
        assert!(matches_extensions(
            Path::new("image.PNG"),
            Some(&extensions)
        ));
        assert!(!matches_extensions(
            Path::new("document.txt"),
            Some(&extensions)
        ));
        assert!(!matches_extensions(Path::new("README"), Some(&extensions)));
        assert!(matches_extensions(Path::new("README"), None));
    }

    #[test]
    fn renames_jpeg_files_with_the_common_lowercase_extension() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "input-file.JPEG", b"hello world");
        let expected = dir
            .path()
            .join(format!("{}.jpg", hash_file(&source).unwrap()));

        let result = process_file(&GlobalOptions::default(), &source).unwrap();

        assert_eq!(result, expected);
        assert!(result.exists());
        assert!(!source.exists());
    }

    #[test]
    fn renames_extensionless_files_without_error() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "example", b"hello world");
        let expected = dir.path().join(hash_file(&source).unwrap());

        let result = process_file(&GlobalOptions::default(), &source).unwrap();

        assert_eq!(result, expected);
        assert!(result.exists());
        assert!(!source.exists());
    }

    #[test]
    fn copies_files_when_copy_mode_is_enabled() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "example.txt", b"hello world");
        let expected = dir
            .path()
            .join(format!("{}.txt", hash_file(&source).unwrap()));

        let opts = GlobalOptions {
            copy: true,
            ..GlobalOptions::default()
        };
        let result = process_file(&opts, &source).unwrap();

        assert_eq!(result, expected);
        assert!(source.exists());
        assert!(result.exists());
    }

    #[test]
    fn moves_files_into_the_output_directory() {
        let dir = TestDir::new();
        let output_dir = dir.path().join("hashed");
        fs::create_dir_all(&output_dir).unwrap();
        let source = write_file(dir.path(), "example.txt", b"hello world");
        let expected = output_dir.join(format!("{}.txt", hash_file(&source).unwrap()));

        let opts = GlobalOptions {
            output_dir: Some(output_dir.clone()),
            ..GlobalOptions::default()
        };
        let result = process_file(&opts, &source).unwrap();

        assert_eq!(result, expected);
        assert!(result.exists());
        assert!(!source.exists());
    }

    #[test]
    fn rejects_existing_destination_without_force_rename() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "example.txt", b"hello world");
        let destination = dir
            .path()
            .join(format!("{}.txt", hash_file(&source).unwrap()));
        fs::write(&destination, b"existing").unwrap();

        let err = process_file(&GlobalOptions::default(), &source).unwrap_err();

        assert!(matches!(err, ProcessFileError::AlreadyExists));
        assert!(source.exists());
        assert!(destination.exists());
    }

    #[test]
    fn skips_already_hashed_filenames_without_force_rehash() {
        let dir = TestDir::new();
        let source = write_file(
            dir.path(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.txt",
            b"hello world",
        );

        let err = process_file(&GlobalOptions::default(), &source).unwrap_err();

        assert!(matches!(err, ProcessFileError::AlreadyProcessed));
        assert!(source.exists());
    }
}
