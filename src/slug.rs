pub fn page_title_to_slug(title: &str) -> String {
    title.replace(' ', "_")
}
