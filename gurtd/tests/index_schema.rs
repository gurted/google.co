use gurtd::index::tantivy::TantivyIndexEngine;
use gurtd::index::IndexEngine;

#[test]
fn tantivy_schema_contains_required_fields() {
    let (schema, fields) = TantivyIndexEngine::build_schema();
    // Resolve field entries by handle and assert names exist in the schema.
    let url = schema.get_field_name(fields.url);
    let domain = schema.get_field_name(fields.domain);
    let title = schema.get_field_name(fields.title);
    let content = schema.get_field_name(fields.content);
    let fetch_time = schema.get_field_name(fields.fetch_time);
    let language = schema.get_field_name(fields.language);
    let render_mode = schema.get_field_name(fields.render_mode);

    assert_eq!(url, "url");
    assert_eq!(domain, "domain");
    assert_eq!(title, "title");
    assert_eq!(content, "content");
    assert_eq!(fetch_time, "fetch_time");
    assert_eq!(language, "language");
    assert_eq!(render_mode, "render_mode");
}

#[test]
fn engine_name_is_tantivy() {
    let engine = TantivyIndexEngine::with_default_schema();
    assert_eq!(engine.engine_name(), "tantivy");
}
