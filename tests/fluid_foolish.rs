//! Opt-in compatibility check against a separately downloaded pack. No pack
//! assets or derived animation fixtures are redistributed.
use punchy_engine::{
    moves_animation::{Controller, Input},
    moves_pack::Pack,
};
#[test]
fn external_fluid_foolish_format_and_samples() {
    let Some(path) = std::env::var_os("PUNCHY_MOVES_PACK_ROOT") else {
        return;
    };
    let pack = Pack::load(std::path::Path::new(&path)).unwrap();
    assert!(pack.warnings.is_empty(), "{:?}", pack.warnings);
    assert_eq!(pack.clips.len(), 268);
    assert_eq!(pack.config["format_version"], 2);
    assert_eq!(pack.emotes().len(), 4);
    for clip in pack.clips.values() {
        for bone in clip.tracks.keys() {
            for t in [0., clip.length / 2., clip.length] {
                let sample = clip.sample_model(bone, t);
                assert!(
                    sample.position.is_finite()
                        && sample.rotation.is_finite()
                        && sample.scale.is_finite(),
                    "{} / {}",
                    clip.name,
                    bone
                );
            }
        }
    }
    let mut c = Controller::default();
    let mut i = Input {
        grounded: true,
        ..Default::default()
    };
    for _ in 0..10 {
        c.tick(&pack, &i);
    }
    assert!(c.frame.layers.iter().any(|l| l.clip.name == "idle"));
    i.movement = [0., 1.];
    i.speed = 0.1;
    for _ in 0..10 {
        c.tick(&pack, &i);
    }
    assert!(
        c.frame
            .layers
            .iter()
            .any(|l| l.clip.name == "normal_forward")
    );
    i.speed = 0.;
    i.movement = [0., 0.];
    c.tick(&pack, &i);
    assert!(c.play_emote(&pack, "shuuba_dance"));
    c.tick(&pack, &i);
    assert!(c.frame.layers.iter().any(|l| l.clip.name == "shuuba_dance"));
}
