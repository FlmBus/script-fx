//! Runs in its own test binary: it changes the *process* locale like Qt hosts do.
use std::path::Path;
use ui_components::{Inputs, Ui};

#[test]
fn numbers_stay_c_formatted_under_a_comma_locale() {
    let ok = unsafe { !libc::setlocale(libc::LC_ALL, c"de_DE.UTF-8".as_ptr()).is_null() };
    if !ok {
        eprintln!("de_DE.UTF-8 not installed, skipping");
        return;
    }
    // Sanity: the C library itself now formats with a comma.
    let mut buf = [0u8; 16];
    unsafe { libc::snprintf(buf.as_mut_ptr().cast(), 16, c"%.2f".as_ptr(), 0.5f64) };
    assert!(std::str::from_utf8(&buf).unwrap().starts_with("0,50"), "locale not active");

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/numbers");
    let mut ui = Ui::load(&dir).unwrap();
    let none = |_: &str| None;
    let none_f = |_: &str| None;
    let none_s = |_: &str| None;
    ui.update(&Inputs { time: 0.0, hover: None, active: None, bound: &none, meter: &none_f, series: &none_s }, 1.0);
    let svg: String = ui.layer_svgs().into_iter().map(|(_, s)| s).collect();
    assert!(svg.contains(r#"opacity="0.08""#), "{svg}");
    assert!(svg.contains(r#"opacity="0.25""#), "{svg}");
    assert!(svg.contains(r#"cx="5.50""#), "{svg}");
    assert!(!svg.contains(",0") && !svg.contains(",5"), "{svg}");
    assert!(ui.errors().is_empty());
    // Restored afterwards: the host's locale is untouched.
    unsafe { libc::snprintf(buf.as_mut_ptr().cast(), 16, c"%.2f".as_ptr(), 0.5f64) };
    assert!(std::str::from_utf8(&buf).unwrap().starts_with("0,50"));
}
