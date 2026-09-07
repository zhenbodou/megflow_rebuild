use macro_lab_derive::Describe;
use std::marker::PhantomData;

struct NotDisplay;
#[derive(Describe)]
struct Packet<'a, T = u32, const N: usize = 4>
where
    T: Copy,
{
    #[describe(rename = "载荷")]
    value: T,
    source: &'a str,
    #[describe(skip)]
    _buffer: [u8; N],
    #[describe(skip)]
    _hidden: PhantomData<NotDisplay>,
}
#[derive(Describe)]
struct Tuple(u32, #[describe(skip)] NotDisplay);
#[derive(Describe)]
struct Empty;
#[derive(Describe)]
struct Marker<T>(#[describe(skip)] PhantomData<T>);

#[test]
fn supports_generics_shapes_and_selective_bounds() {
    let p = Packet::<u32, 4> {
        value: 7,
        source: "inp",
        _buffer: [0; 4],
        _hidden: PhantomData,
    };
    assert_eq!(p.describe(), "载荷=7, source=inp");
    assert_eq!(Tuple(3, NotDisplay).describe(), "0=3");
    assert_eq!(Empty.describe(), "");
    assert_eq!(Marker::<NotDisplay>(PhantomData).describe(), "");
}
