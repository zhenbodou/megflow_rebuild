use macro_lab_derive::Describe;

#[derive(Describe)]
struct Conditional {
    value: u32,
    #[cfg(feature = "metrics")]
    samples: u32,
}

#[test]
fn derive_observes_enabled_fields() {
    let value = Conditional {
        value: 7,
        #[cfg(feature = "metrics")]
        samples: 2,
    };
    #[cfg(feature = "metrics")]
    assert_eq!(value.describe(), "value=7, samples=2");
    #[cfg(not(feature = "metrics"))]
    assert_eq!(value.describe(), "value=7");
}
