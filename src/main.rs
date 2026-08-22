use argparse::{ArgumentParser, Collect, List, StoreOption, StoreTrue};
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
    recursive: bool,
    version: bool,
    extensions: Vec<String>,
    copy_to: Option<PathBuf>,
    move_to: Option<PathBuf>,
    paths: Vec<String>,
}

#[derive(Debug)]
enum ProcessFileError {
    AlreadyExists(PathBuf),
    AlreadyProcessed,
    InvalidPathComponent(&'static str),
    Io(io::Error),
    NotAFile,
    Hash(Box<dyn Error>),
}

impl fmt::Display for ProcessFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(path) => {
                write!(f, "Already exists: \"{}\"", path.display())
            }
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

    if let Err(message) = validate_options(&opts) {
        eprintln!("hashname: {message}");
        std::process::exit(2);
    }

    if !opts.dry_run
        && let Some(output_dir) = output_dir(&opts)
        && let Err(err) = fs::create_dir_all(output_dir)
    {
        eprintln!(
            "Could not create output directory \"{}\": {err}",
            output_dir.display()
        );
        std::process::exit(1);
    }

    let extensions = parse_extensions(&opts.extensions);
    filter_files(
        &opts.paths,
        extensions.as_deref(),
        opts.recursive,
        opts.verbose,
    )
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
        &["-n", "--dry-run"],
        StoreTrue,
        "Preview operations without changing files",
    );
    ap.refer(&mut opts.force_rehash).add_option(
        &["-f", "--force-rehash"],
        StoreTrue,
        "Process the file even if it looks like it has already been processed",
    );
    ap.refer(&mut opts.force_rename).add_option(
        &["-F", "--force-rename"],
        StoreTrue,
        "Overwrite an existing destination file",
    );
    ap.refer(&mut opts.extensions).add_option(
        &["-e", "--extension"],
        Collect,
        "Process this extension; repeatable or comma-separated",
    );
    ap.refer(&mut opts.recursive).add_option(
        &["-r", "--recursive"],
        StoreTrue,
        "Traverse directory inputs recursively",
    );
    ap.refer(&mut opts.copy_to).add_option(
        &["--copy-to"],
        StoreOption,
        "Copy renamed files into this directory",
    );
    ap.refer(&mut opts.move_to).add_option(
        &["--move-to"],
        StoreOption,
        "Move renamed files into this directory",
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
    ap.refer(&mut opts.paths)
        .add_argument("path", List, "Files or directories to process");
    ap.parse_args_or_exit();
}

fn validate_options(opts: &GlobalOptions) -> Result<(), &'static str> {
    if opts.copy_to.is_some() && opts.move_to.is_some() {
        return Err("--copy-to and --move-to cannot be used together");
    }
    Ok(())
}

fn output_dir(opts: &GlobalOptions) -> Option<&Path> {
    opts.copy_to.as_deref().or(opts.move_to.as_deref())
}

fn filter_files(
    paths: &[String],
    extensions: Option<&[String]>,
    recursive: bool,
    verbose: bool,
) -> Vec<PathBuf> {
    let mut valid_files = Vec::new();

    for path in paths {
        let candidate = Path::new(path);
        if candidate.exists() {
            collect_path(candidate, extensions, recursive, verbose, &mut valid_files);
        } else {
            match glob(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(path) => collect_path(
                                &path,
                                extensions,
                                recursive,
                                verbose,
                                &mut valid_files,
                            ),
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

fn collect_path(
    path: &Path,
    extensions: Option<&[String]>,
    recursive: bool,
    verbose: bool,
    files: &mut Vec<PathBuf>,
) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            if verbose {
                eprintln!("Skipped \"{}\": {err}", path.display());
            }
            return;
        }
    };

    if metadata.file_type().is_file() {
        if matches_extensions(path, extensions) {
            files.push(path.to_path_buf());
        }
        return;
    }

    if !metadata.file_type().is_dir() {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) => {
            if verbose {
                eprintln!("Skipped directory \"{}\": {err}", path.display());
            }
            return;
        }
    };

    for entry in entries {
        match entry {
            Ok(entry) => {
                let entry_path = entry.path();
                match entry.file_type() {
                    Ok(file_type) if file_type.is_file() => {
                        if matches_extensions(&entry_path, extensions) {
                            files.push(entry_path);
                        }
                    }
                    Ok(file_type) if recursive && file_type.is_dir() => {
                        collect_path(&entry_path, extensions, true, verbose, files);
                    }
                    Ok(_) => {}
                    Err(err) if verbose => {
                        eprintln!("Skipped \"{}\": {err}", entry_path.display());
                    }
                    Err(_) => {}
                }
            }
            Err(err) if verbose => {
                eprintln!("Skipped directory entry in \"{}\": {err}", path.display());
            }
            Err(_) => {}
        }
    }
}

fn parse_extensions(values: &[String]) -> Option<Vec<String>> {
    if values.is_empty() {
        return None;
    }

    Some(
        values
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .map(|extension| extension.trim_start_matches('.'))
            .filter(|extension| !extension.is_empty())
            .map(normalize_extension)
            .collect(),
    )
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
        _ => result_hash.clone(),
    };

    let result_path = match output_dir(opts) {
        Some(output_dir) => output_dir.join(&result_filename),
        None => raw_path.with_file_name(&result_filename),
    };

    if result_path.exists() {
        let is_move_duplicate = opts.move_to.is_some()
            && !paths_refer_to_same_file(raw_path, &result_path)
            && hash_file(&result_path)? == result_hash;

        if is_move_duplicate {
            if !opts.dry_run {
                fs::remove_file(raw_path)?;
            }
            return Ok(result_path);
        }

        if !opts.force_rename {
            return Err(ProcessFileError::AlreadyExists(result_path));
        }
    }

    if !opts.dry_run {
        if opts.copy_to.is_some() {
            fs::copy(raw_path, &result_path)?;
        } else {
            move_file(raw_path, &result_path)?;
        }
    }

    Ok(result_path)
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn move_file(source: &Path, destination: &Path) -> io::Result<()> {
    move_file_with(source, destination, |source, destination| {
        fs::rename(source, destination)
    })
}

fn move_file_with<F>(source: &Path, destination: &Path, rename: F) -> io::Result<()>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    match rename(source, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
            fs::copy(source, destination)?;
            fs::remove_file(source)
        }
        Err(err) => Err(err),
    }
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
    fn parses_repeated_and_comma_separated_extensions() {
        assert_eq!(
            parse_extensions(&[" JPG, .PnG".to_owned(), "jpeg ".to_owned()]),
            Some(vec!["jpg".to_owned(), "png".to_owned(), "jpg".to_owned()])
        );
        assert_eq!(parse_extensions(&[]), None);
    }

    #[test]
    fn matches_only_requested_extensions() {
        let extensions = parse_extensions(&["jpg,png".to_owned()]).unwrap();

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
    fn finds_files_in_directory_inputs() {
        let dir = TestDir::new();
        let top_level = write_file(dir.path(), "top-level.JPG", b"top level");
        write_file(dir.path(), "ignored.txt", b"ignored");
        let nested_dir = dir.path().join("nested");
        fs::create_dir(&nested_dir).unwrap();
        let nested = write_file(&nested_dir, "nested.png", b"nested");
        let paths = vec![dir.path().to_string_lossy().into_owned()];
        let extensions = parse_extensions(&["jpg,png".to_owned()]).unwrap();

        let mut shallow = filter_files(&paths, Some(&extensions), false, false);
        shallow.sort();
        assert_eq!(shallow, vec![top_level.clone()]);

        let mut recursive = filter_files(&paths, Some(&extensions), true, false);
        recursive.sort();
        let mut expected = vec![top_level, nested];
        expected.sort();
        assert_eq!(recursive, expected);
    }

    #[test]
    fn rejects_multiple_destination_modes() {
        let opts = GlobalOptions {
            copy_to: Some(PathBuf::from("copied")),
            move_to: Some(PathBuf::from("moved")),
            ..GlobalOptions::default()
        };

        assert_eq!(
            validate_options(&opts),
            Err("--copy-to and --move-to cannot be used together")
        );
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
            copy_to: Some(dir.path().to_path_buf()),
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
            move_to: Some(output_dir.clone()),
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

        assert!(matches!(
            err,
            ProcessFileError::AlreadyExists(ref path) if path == &destination
        ));
        assert_eq!(
            err.to_string(),
            format!("Already exists: \"{}\"", destination.display())
        );
        assert!(source.exists());
        assert!(destination.exists());
    }

    #[test]
    fn removes_move_source_when_identical_destination_exists() {
        let dir = TestDir::new();
        let output_dir = dir.path().join("hashed");
        fs::create_dir(&output_dir).unwrap();
        let source = write_file(dir.path(), "example.txt", b"hello world");
        let destination = output_dir.join(format!("{}.txt", hash_file(&source).unwrap()));
        fs::write(&destination, b"hello world").unwrap();
        let opts = GlobalOptions {
            move_to: Some(output_dir),
            ..GlobalOptions::default()
        };

        let result = process_file(&opts, &source).unwrap();

        assert_eq!(result, destination);
        assert!(!source.exists());
        assert_eq!(fs::read(destination).unwrap(), b"hello world");
    }

    #[test]
    fn keeps_move_source_when_existing_destination_differs() {
        let dir = TestDir::new();
        let output_dir = dir.path().join("hashed");
        fs::create_dir(&output_dir).unwrap();
        let source = write_file(dir.path(), "example.txt", b"hello world");
        let destination = output_dir.join(format!("{}.txt", hash_file(&source).unwrap()));
        fs::write(&destination, b"different contents").unwrap();
        let opts = GlobalOptions {
            move_to: Some(output_dir),
            ..GlobalOptions::default()
        };

        let err = process_file(&opts, &source).unwrap_err();

        assert!(matches!(
            err,
            ProcessFileError::AlreadyExists(ref path) if path == &destination
        ));
        assert!(source.exists());
        assert_eq!(fs::read(destination).unwrap(), b"different contents");
    }

    #[test]
    fn falls_back_to_copy_and_delete_for_cross_device_moves() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "source.txt", b"hello world");
        let destination = dir.path().join("destination.txt");

        move_file_with(&source, &destination, |_, _| {
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        })
        .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(destination).unwrap(), b"hello world");
    }

    #[test]
    fn keeps_source_when_cross_device_copy_fails() {
        let dir = TestDir::new();
        let source = write_file(dir.path(), "source.txt", b"hello world");
        let destination = dir.path().join("missing").join("destination.txt");

        let err = move_file_with(&source, &destination, |_, _| {
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        })
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(source.exists());
        assert!(!destination.exists());
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
