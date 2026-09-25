//! The compositor's shader parses and validates, so a broken kind fails a
//! test instead of the first frame.
#[test]
fn quad_shader_validates() {
    let source = include_str!("../src/quad.wgsl");
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
}
