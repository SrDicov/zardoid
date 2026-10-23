use std::process::Command;

fn z() -> Command {
    Command::new(env!("CARGO_BIN_EXE_zardoid"))
}

#[test]
fn help_sale_cero() {
    let out = z().arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Uso:"));
}

#[test]
fn sin_url_es_uso_invalido() {
    let out = z().output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn json_sin_out_es_uso_invalido() {
    let out = z()
        .args(["https://github.com/o/r", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn opcion_desconocida_es_uso_invalido() {
    let out = z()
        .args(["https://github.com/o/r", "--nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn url_no_reconocida_es_uso_invalido() {
    let out = z().arg("basura").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}
