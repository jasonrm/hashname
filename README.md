# hashname

```
❯ hashname --help
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
  -o,--output-dir OUTPUT_DIR
                        Renamed files are moved to this directory
  -c,--copy             Copy files to new name instead of moving
  -v,--verbose          Print more information during processing
  -V,--version          Print version and exit
```

## Examples

```
hashname --dry-run doc1.pdf game.iso img.png
```

```
hashname --dry-run '*.pdf'
```

```
hashname --dry-run --output-dir ./hashed/ '*.png'
```
