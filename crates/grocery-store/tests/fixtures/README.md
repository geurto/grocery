Synthetic files in the NEVO-online CSV layout. Food names follow NEVO's
naming style, but every number is invented. Don't replace them with real NEVO
rows: the dataset is RIVM's and is not redistributed in this repository.

To test against the real export, run the ignored test:

    NEVO_DIR=/path/to/unzipped/NEVO2025 cargo test -p grocery-store -- --ignored
