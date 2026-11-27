use thumbnail_consumer::{Thumbnail, Sticker, StickerError, Bounds};
#[test]
fn shared_wire() {
    assert_eq!(Bounds::PIXELS, 4);
    let wire = Thumbnail::write(&[0,255]).unwrap();
    assert_eq!(Sticker::read(&wire).unwrap(), [0,255]);
    assert_eq!(Sticker::read(b"{}"), Err(StickerError::Malformed));
}
