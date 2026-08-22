# hashname

`hashname` renames files to their SHA-256 hash.

It is useful when you want stable, content-based filenames for a set of files. By default it moves each file in place, preserving the original extension when there is one. Extensions are normalized to lowercase, and JPEG aliases such as `.jpeg` and `.jpe` are normalized to the more common `.jpg`.

## What It Does

Given a file like `photo.jpg`, `hashname` will rename it to something like:

```text
0f4636c78f65d3639ece5a064b5ae753e3408614a14fb18ab4d7540d2c248543.jpg
```

For example, `input-file.JPEG` becomes `<hash>.jpg`. If the file has no extension, the result is just the hash.

## Usage

```text
hashname [OPTIONS] [PATH ...]
```

Paths can be files or directories. Directory inputs process their immediate files by default; use `--recursive` to include nested directories. Glob patterns such as `'*.pdf'` are also supported.

## Common Options

- `-e, --extension EXT`: process this extension; repeat the option or use commas for multiple extensions
- `-r, --recursive`: traverse directory inputs recursively
- `-n, --dry-run`: preview operations without changing files
- `--copy-to DIR`: copy renamed files into `DIR`
- `--move-to DIR`: move renamed files into `DIR`
- `--force-rehash`: process files even if the filename already looks like a hash
- `--force-rename`: overwrite an existing destination path
- `--verbose`: print skipped files and the reason they were skipped

Extension matching is case-insensitive, accepts entries with or without a leading dot, and treats JPEG aliases as `.jpg`. `--copy-to` and `--move-to` create the destination directory when needed and cannot be used together.

## Examples

Rename all files in the current directory:

```sh
hashname .
```

Rename JPEG and PNG files in the current directory:

```sh
hashname . -e jpg -e png
```

Process JPEG and PNG files recursively:

```sh
hashname photos -e jpg,png -r
```

Copy JPEG files into a separate directory with hashed names:

```sh
hashname photos -e jpg --copy-to hashed
```

Preview an operation without changing files:

```sh
hashname photos -e jpg -n
```

## Help Output

```text
Usage:
  hashname [OPTIONS] [PATH ...]

Rename files to their hash

Positional arguments:
  path                  Files or directories to process

Optional arguments:
  -h,--help             Show this help message and exit
  -n,--dry-run          Preview operations without changing files
  -f,--force-rehash     Process the file even if it looks like it has already
                        been processed
  -F,--force-rename     Overwrite an existing destination file
  -e,--extension EXTENSION
                        Process this extension; repeatable or comma-separated
  -r,--recursive        Traverse directory inputs recursively
  --copy-to COPY_TO     Copy renamed files into this directory
  --move-to MOVE_TO     Move renamed files into this directory
  -v,--verbose          Print more information during processing
  -V,--version          Print version and exit
```
