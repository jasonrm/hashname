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
hashname [OPTIONS] [FILE ...]
```

You can pass file paths directly or use glob patterns such as `'*.pdf'`.

## Common Options

- `--dry-run`: show what would happen without renaming or copying files
- `--copy`: copy files to their hashed names instead of moving them
- `--output-dir DIR`: write the hashed files into another directory
- `--extensions LIST`: only process files matching a comma-separated list of extensions, case-insensitively
- `--force-rehash`: process files even if the filename already looks like a hash
- `--force-rename`: overwrite an existing destination path
- `--verbose`: print skipped files and the reason they were skipped

## Examples

Preview a few files:

```sh
hashname --dry-run doc1.pdf game.iso img.png
```

Preview all JPEG and PNG files in the current directory:

```sh
hashname --dry-run --extensions jpg,png '*'
```

Extension matching is case-insensitive, accepts entries with or without a leading dot, and treats JPEG aliases as `.jpg`.

Copy PNG files into a separate directory with hashed names:

```sh
hashname --copy --output-dir ./hashed '*.png'
```

Rename files in place:

```sh
hashname report.pdf image.png
```

## Help Output

```text
Usage:
  hashname [OPTIONS] [FILE ...]

Rename files to their hash

Positional arguments:
  file                  Files to process

Optional arguments:
  -h,--help             Show this help message and exit
  -d,--dry-run          Do not actually rename files
  -f,--force-rehash     Process the file even if it looks like it has already
                        been processed
  -F,--force-rename     Rename file even there is another file with the same
                        result name
  --extensions EXTENSIONS
                        Only process files with an extension in this
                        comma-separated list
  -o,--output-dir OUTPUT_DIR
                        Renamed files are moved to this directory
  -c,--copy             Copy files to new name instead of moving
  -v,--verbose          Print more information during processing
  -V,--version          Print version and exit
```
