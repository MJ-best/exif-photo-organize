//! Scene categories (portrait, landscape, ...) from faces and an ImageNet classifier.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum Scene {
    #[default]
    Unknown = 0,
    Portrait = 1,
    Landscape = 2,
    City = 3,
    StillLife = 4,
    Food = 5,
    Animal = 6,
    Other = 7,
}

impl Scene {
    pub const ALL: [Scene; 7] =
        [Scene::Portrait, Scene::Landscape, Scene::City, Scene::StillLife, Scene::Food, Scene::Animal, Scene::Other];

    pub fn label(self) -> &'static str {
        match self {
            Scene::Unknown => "미분석",
            Scene::Portrait => "인물",
            Scene::Landscape => "풍경",
            Scene::City => "도시·건축",
            Scene::StillLife => "정물·사물",
            Scene::Food => "음식",
            Scene::Animal => "동물",
            Scene::Other => "기타",
        }
    }

    pub fn from_u8(v: u8) -> Scene {
        match v {
            1 => Scene::Portrait,
            2 => Scene::Landscape,
            3 => Scene::City,
            4 => Scene::StillLife,
            5 => Scene::Food,
            6 => Scene::Animal,
            7 => Scene::Other,
            _ => Scene::Unknown,
        }
    }

    pub fn from_label(s: &str) -> Option<Scene> {
        Scene::ALL.iter().copied().find(|x| x.label() == s)
    }
}

/// ImageNet-1k class index → scene category.
pub fn class_scene(i: usize) -> Scene {
    const CITY: &[usize] = &[
        410, 415, 424, 425, 437, 442, 449, 454, 460, 467, 483, 497, 498, 500, 509, 525, 538, 562, 580, 582, 624, 663,
        668, 682, 698, 706, 718, 727, 743, 762, 788, 821, 825, 829, 832, 839, 858, 860, 865, 873, 874, 888, 900, 919,
        920,
    ];
    const PEOPLE: &[usize] = &[981, 982, 983];
    match i {
        970 | 972..=980 | 958 | 984 | 912 => Scene::Landscape,
        0..=397 => Scene::Animal,
        923..=969 => Scene::Food,
        _ if CITY.contains(&i) => Scene::City,
        _ if PEOPLE.contains(&i) => Scene::Portrait,
        398..=922 | 985..=999 | 971 => Scene::StillLife,
        _ => Scene::Other,
    }
}

/// Probability mass per scene from ImageNet class probabilities.
pub fn scene_mass(probs: &[f32]) -> [f32; 8] {
    let mut m = [0f32; 8];
    for (i, p) in probs.iter().enumerate() {
        m[class_scene(i) as usize] += *p;
    }
    m
}

/// Decide the scene. `face_area` is the largest face's share of the image area,
/// `faces` the number of reasonably sized faces.
pub fn decide(face_area: f32, faces: usize, probs: Option<&[f32]>) -> (Scene, f32) {
    // 0.4 % of the frame ≈ a full-body shot of one person; smaller faces are passers-by.
    if face_area >= 0.004 || faces >= 2 {
        let conf = (0.6 + face_area * 10.0).min(0.99);
        return (Scene::Portrait, conf);
    }
    let Some(probs) = probs else { return (Scene::Other, 0.0) };
    let m = scene_mass(probs);
    let (best, mass) =
        m.iter().enumerate().skip(1).fold((0usize, 0f32), |acc, (i, v)| if *v > acc.1 { (i, *v) } else { acc });
    if mass < 0.25 {
        return (Scene::Other, mass);
    }
    let s = Scene::from_u8(best as u8);
    // Weak person evidence without a detected face (back views, small people) stays "Other".
    if s == Scene::Portrait {
        return (Scene::Other, mass);
    }
    (s, mass)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping() {
        assert_eq!(class_scene(207), Scene::Animal); // golden retriever
        assert_eq!(class_scene(963), Scene::Food); // pizza
        assert_eq!(class_scene(970), Scene::Landscape); // alp
        assert_eq!(class_scene(483), Scene::City); // castle
        assert_eq!(class_scene(805), Scene::StillLife); // soccer ball
        assert_eq!(decide(0.02, 1, None).0, Scene::Portrait);
        let mut p = vec![0f32; 1000];
        p[978] = 0.7; // seashore
        assert_eq!(decide(0.0, 0, Some(&p)).0, Scene::Landscape);
    }
}
