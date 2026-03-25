fn main() {
    let output = std::process::Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output();

    let version = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "v0.0.0".to_string(),
    };

    println!("cargo:rustc-env=GIT_VERSION={}", version);
}
