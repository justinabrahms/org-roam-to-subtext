# Org -> subtext

This is intended as a tool to take in org files (specifically those built around org-roam) and serialize them out to [subtext](https://github.com/subconsciousnetwork/subtext).

## Usage

```bash
DATABASE_URL="sqlite:$ORG_ROAM_DB_LOCATION" \
org-to-subtext --filename "$ORG_FILE" -o "$SUBTEXT_OUTFILE"
```

If you pass the `--debug` flag, you can see the resulting parse tree which is useful for development.
