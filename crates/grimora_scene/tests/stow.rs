//! The places on the body and the clips that go to them. `STOW_NUMBERS=1 ... -- --nocapture` prints
//! what `tools/gen_stow_clips.py` needs to know about the rig.

use grimora_core::Rig;
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, StowBook, load_model};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const STOW: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest_stow.ron");

#[test]
fn numbers_for_the_generator() {
    if std::env::var("STOW_NUMBERS").is_err() {
        return;
    }
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    let (hand_rest, weapon, fingers) = rig.hand_numbers();
    println!("hand_rest {:?}", hand_rest.to_cols_array_2d());
    println!("weapon {weapon:?} fingers {fingers:?}");
}

#[test]
fn every_place_has_its_clips_and_has_the_thing_where_the_clip_leaves_it() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let book = StowBook::load(STOW).unwrap();
    assert!(!book.places.is_empty());
    for place in &book.places {
        for clip in [place.draw(), place.stow()] {
            let at = skeleton.clip(&clip).unwrap_or_else(|| panic!("no clip {clip}"));
            assert!(skeleton.clips[at].duration > 0.3);
        }
        assert!(place.frame(skeleton, place.grab).is_some(), "{}", place.id);
        let (draw, stow) = (place.frame(skeleton, place.grab).unwrap(), place.frame_stowing(skeleton).unwrap());
        // Both clips leave it in one place.
        let moved = (draw.w_axis - stow.w_axis).truncate().length();
        assert!(moved < 0.01, "{}: draw and stow disagree by {moved} m", place.id);
    }
}
