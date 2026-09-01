fn before() {}
#[cfg(test)]
mod outer {
    mod inner { fn hidden() {} }
    #[cfg(test)] mod nested { fn hidden_too() {} }
}
fn after() {}
