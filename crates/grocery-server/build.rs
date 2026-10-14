// Re-embed the web app when `web/dist` changes (a new `npm run build`):
// cargo doesn't otherwise notice files appearing in a directory it embeds.
fn main() {
    println!("cargo:rerun-if-changed=../../web/dist");
}
