//! The people of the village: who lives in each house, made from the seed alone. Each has a
//! name, a trade, an age, ties to the others of the house, a bed and a seat at the table.

use grimora_scene::Trade;

use crate::{GenId, path_seed};

/// How the person stands to the head of the house.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kin {
    Head,
    Spouse,
    Child,
    /// A parent of the head.
    Elder,
    Sibling,
}

/// What a person is doing at an hour of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Doing {
    Sleep,
    /// At the house's table.
    Eat,
    /// At their [`Post`].
    Work,
    /// At the tavern or on the largo ([`Resident::leisure`]).
    Leisure,
    /// In the house, awake.
    Home,
}

/// Where someone spends the working hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Post {
    /// The house's trade's place: forge and anvil, the bank and the boat, the counter, the
    /// healer's table, the largo for the chief, the field and the clotheslines for a plain home.
    Trade,
    /// Chores in the house.
    Home,
    Field,
    /// Children play there.
    Largo,
}

/// When someone does what, hours of the day (0..24).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Day {
    pub wake: f32,
    pub work: f32,
    pub lunch: f32,
    pub done: f32,
    pub home: f32,
    pub sleep: f32,
}

impl Day {
    /// What one does at `hour`.
    pub fn doing(&self, hour: f32) -> Doing {
        let h = hour.rem_euclid(24.0);
        if h < self.wake || h >= self.sleep {
            Doing::Sleep
        } else if h < self.work || (h >= self.lunch && h < self.lunch + 1.0) {
            Doing::Eat
        } else if h < self.done {
            Doing::Work
        } else if h < self.home {
            Doing::Leisure
        } else {
            Doing::Home
        }
    }
}

/// One person of a house.
#[derive(Debug, Clone, PartialEq)]
pub struct Resident {
    pub id: GenId,
    pub name: String,
    pub woman: bool,
    pub age: u32,
    pub kin: Kin,
    /// Their house's trade.
    pub trade: Trade,
    /// What they say they are: "ferreiro", "filha do pescador"...
    pub role: String,
    /// The others of the house and what each is to this one ("marido", "filho"...).
    pub ties: Vec<(GenId, String)>,
    /// Which of the house's beds (a couple or a bunk shares one); `None` only in a house with none.
    pub bed: Option<usize>,
    /// Their seat at the house's table.
    pub seat: usize,
    /// Body height relative to the model (children are smaller).
    pub height: f32,
    /// What tints the model, sRGB.
    pub tint: [f32; 3],
    pub post: Post,
    /// Leisure at the tavern (else on the largo).
    pub tavern: bool,
    pub day: Day,
}

const MEN: &[&str] = &[
    "Aurélio",
    "Bento",
    "Caetano",
    "Damião",
    "Estevão",
    "Fernão",
    "Gaspar",
    "Inácio",
    "Jerônimo",
    "Lourenço",
    "Matias",
    "Nuno",
    "Pascoal",
    "Quirino",
    "Rodrigo",
    "Salvador",
    "Tadeu",
    "Vicente",
    "Baltasar",
    "Cosme",
    "Elói",
    "Tomé",
    "Brás",
    "Leandro",
];
const WOMEN: &[&str] = &[
    "Adelaide",
    "Brites",
    "Catarina",
    "Damiana",
    "Esperança",
    "Filipa",
    "Graça",
    "Inês",
    "Joana",
    "Leonor",
    "Mécia",
    "Olímpia",
    "Perpétua",
    "Quitéria",
    "Rosaura",
    "Teodora",
    "Úrsula",
    "Violante",
    "Benedita",
    "Clara",
    "Isaura",
    "Guiomar",
    "Aldonça",
    "Feliciana",
];
const FAMILIES: &[&str] = &[
    "Arianes",
    "Varela",
    "Mondragão",
    "Quental",
    "Salvaterra",
    "Brandão",
    "Teixedo",
    "Orvalho",
    "Valdoro",
    "Ferraz",
    "Aldemar",
    "Corvelo",
    "Keldorn",
    "Sarrazim",
    "Montalvo",
    "Bezerra",
    "Lagoa",
    "Tavares",
];
/// Muted tints over the model's own colors.
const TINTS: &[[f32; 3]] = &[
    [1.0, 1.0, 1.0],
    [0.95, 0.86, 0.76],
    [0.82, 0.72, 0.62],
    [0.72, 0.62, 0.54],
    [1.0, 0.93, 0.86],
    [0.86, 0.82, 0.9],
    [0.9, 0.95, 0.85],
    [0.78, 0.8, 0.86],
];

/// A number from 0 to 1 for the question `what` about `id`.
fn unit(id: u64, what: u64) -> f64 {
    (path_seed(&[id, what]) >> 11) as f64 / (1u64 << 53) as f64
}

fn pick<'a>(id: u64, what: u64, list: &[&'a str]) -> &'a str {
    list[(path_seed(&[id, what]) % list.len() as u64) as usize]
}

fn title(trade: Trade, woman: bool) -> &'static str {
    match (trade, woman) {
        (Trade::Fisher, false) => "pescador",
        (Trade::Fisher, true) => "pescadora",
        (Trade::Smith, false) => "ferreiro",
        (Trade::Smith, true) => "ferreira",
        (Trade::Tavern, false) => "taverneiro",
        (Trade::Tavern, true) => "taverneira",
        (Trade::Healer, false) => "curandeiro",
        (Trade::Healer, true) => "curandeira",
        (Trade::Chief, _) => "chefe da vila",
        (Trade::Home, false) => "lavrador",
        (Trade::Home, true) => "lavradora",
        (Trade::Herder, false) => "pastor",
        (Trade::Herder, true) => "pastora",
        (Trade::Sexton, false) => "coveiro",
        (Trade::Sexton, true) => "coveira",
        (Trade::Carpenter, false) => "carpinteiro",
        (Trade::Carpenter, true) => "carpinteira",
    }
}

/// The people of the house `house` (its lot's id) of `trade` whose beds sleep `sleeps` each
/// (the first is the head's): one to four, never more than the beds hold. The same for the
/// same house.
pub fn residents(house: GenId, trade: Trade, sleeps: &[usize]) -> Vec<Resident> {
    let h = house.0;
    // A place in a bed for each, in order: the head and a spouse share the first.
    let places: Vec<usize> = sleeps.iter().enumerate().flat_map(|(bed, &n)| std::iter::repeat_n(bed, n)).collect();
    let room = places.len().clamp(1, 4);
    let count = if room > 1 && unit(h, 4) < 0.2 { room - 1 } else { room };
    let couple = sleeps.first().is_some_and(|&n| n >= 2);
    let family = pick(h, 1, FAMILIES);
    let head_woman = match trade {
        Trade::Healer => unit(h, 2) < 0.85,
        Trade::Fisher | Trade::Smith | Trade::Chief | Trade::Carpenter | Trade::Sexton => unit(h, 2) < 0.15,
        _ => unit(h, 2) < 0.4,
    };
    let head_age = 24 + (unit(h, 3) * 34.0) as u32;
    // Who lives with the head, before names.
    let mut people = vec![(Kin::Head, head_woman, head_age)];
    let mut has_elder = false;
    for i in 1..count {
        let k = 100 + i as u64 * 10;
        let roll = unit(h, k);
        let person = if i == 1 && couple && roll < 0.8 {
            (Kin::Spouse, !head_woman, (head_age as i64 + (unit(h, k + 1) * 12.0) as i64 - 6).max(18) as u32)
        } else if !has_elder && roll > 0.85 && head_age < 50 {
            has_elder = true;
            (Kin::Elder, unit(h, k + 2) < 0.6, (head_age + 22 + (unit(h, k + 1) * 8.0) as u32).min(82))
        } else if head_age >= 20 && roll > 0.15 {
            // Born when the head was 18 to 44.
            let (youngest, oldest) = (head_age.saturating_sub(44).max(1), head_age.saturating_sub(18).max(1));
            (Kin::Child, unit(h, k + 2) < 0.5, youngest + (unit(h, k + 1) * (oldest - youngest + 1) as f64) as u32)
        } else {
            (Kin::Sibling, unit(h, k + 2) < 0.5, (head_age as i64 + (unit(h, k + 1) * 14.0) as i64 - 7).max(16) as u32)
        };
        people.push(person);
    }
    let ids: Vec<GenId> = (0..people.len()).map(|i| GenId(path_seed(&[h, 0xf01c, i as u64]))).collect();
    let mut used: Vec<&str> = Vec::new();
    let head_title = title(trade, head_woman);
    people
        .iter()
        .enumerate()
        .map(|(i, &(kin, woman, age))| {
            let id = ids[i];
            let names = if woman { WOMEN } else { MEN };
            let mut n = 0;
            let mut given = pick(id.0, 7, names);
            while used.contains(&given) && n < 8 {
                n += 1;
                given = pick(id.0, 7 + n, names);
            }
            used.push(given);
            let role = match kin {
                Kin::Head => head_title.to_owned(),
                Kin::Spouse => format!("{} {}", if woman { "mulher" } else { "marido" }, of(head_title, head_woman)),
                Kin::Child => format!("{} {}", if woman { "filha" } else { "filho" }, of(head_title, head_woman)),
                Kin::Elder => format!("{} {}", if woman { "mãe" } else { "pai" }, of(head_title, head_woman)),
                Kin::Sibling => format!("{} {}", if woman { "irmã" } else { "irmão" }, of(head_title, head_woman)),
            };
            let ties = people
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(j, &(other, other_woman, _))| (ids[j], tie(kin, other, other_woman).to_owned()))
                .collect();
            let grown = (age.min(16) as f32 / 16.0).max(0.25);
            let height = (0.55 + 0.45 * grown) * (0.94 + unit(id.0, 20) as f32 * 0.12) * if woman { 0.96 } else { 1.0 };
            let tint = TINTS[(path_seed(&[id.0, 21]) % TINTS.len() as u64) as usize];
            let post = match kin {
                Kin::Head | Kin::Sibling => Post::Trade,
                Kin::Spouse if trade == Trade::Home || unit(id.0, 30) < 0.5 => Post::Field,
                Kin::Child if age >= 5 => Post::Largo,
                _ => Post::Home,
            };
            let day = Day {
                wake: 5.0 + unit(id.0, 31) as f32 * 1.5,
                work: 6.8 + unit(id.0, 32) as f32 * 0.7,
                lunch: 11.8 + unit(id.0, 33) as f32 * 0.6,
                done: 16.5 + unit(id.0, 34) as f32 * 1.5,
                home: if age < 12 { 18.5 } else { 19.5 + unit(id.0, 35) as f32 * 2.0 },
                sleep: if age < 12 { 20.0 } else { 21.5 + unit(id.0, 36) as f32 * 1.5 },
            };
            Resident {
                id,
                name: format!("{given} {family}"),
                woman,
                age,
                kin,
                trade,
                role,
                ties,
                bed: places.get(i).copied(),
                seat: i,
                height,
                tint,
                post,
                tavern: age >= 16 && (trade == Trade::Tavern || unit(id.0, 37) < 0.6),
                day,
            }
        })
        .collect()
}

fn of(head_title: &str, head_woman: bool) -> String {
    format!("{} {head_title}", if head_woman { "da" } else { "do" })
}

/// What `other` is to someone who is `me` to the head.
fn tie(me: Kin, other: Kin, woman: bool) -> &'static str {
    let w = |a, b| if woman { b } else { a };
    match (me, other) {
        (Kin::Head, Kin::Spouse) | (Kin::Spouse, Kin::Head) => w("marido", "mulher"),
        (Kin::Head | Kin::Spouse, Kin::Child) => w("filho", "filha"),
        (Kin::Head, Kin::Elder) => w("pai", "mãe"),
        (Kin::Head | Kin::Sibling, Kin::Sibling | Kin::Head) => w("irmão", "irmã"),
        (Kin::Child, Kin::Head | Kin::Spouse) => w("pai", "mãe"),
        (Kin::Child, Kin::Child) => w("irmão", "irmã"),
        (Kin::Child, Kin::Elder) => w("avô", "avó"),
        (Kin::Elder, Kin::Head) => w("filho", "filha"),
        (Kin::Elder, Kin::Child) => w("neto", "neta"),
        _ => w("parente", "parente"),
    }
}

/// What two people of different houses are to each other, besides neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bond {
    /// They look for each other at the tavern and on the largo.
    Friend,
    /// They keep away from each other.
    Foe,
}

/// The bond between `a` and `b`, of different houses, the same whichever comes first:
/// about one pair in five are friends and one in fifteen foes. Children have neither.
pub fn bond(a: &Resident, b: &Resident) -> Option<Bond> {
    if a.age < 14 || b.age < 14 {
        return None;
    }
    let (x, y) = if a.id.0 < b.id.0 { (a.id.0, b.id.0) } else { (b.id.0, a.id.0) };
    let roll = unit(path_seed(&[x, y]), 0xb0d);
    if roll < 0.22 {
        Some(Bond::Friend)
    } else if roll > 0.935 {
        Some(Bond::Foe)
    } else {
        None
    }
}

/// The shape keys of `r`'s body (the chartest's morph targets) and their weights: by sex,
/// age, trade and a little of the seed. The same for the same person.
pub fn body_shape(r: &Resident) -> Vec<(&'static str, f32)> {
    let id = r.id.0;
    let u = |k: u64| unit(id, 200 + k) as f32;
    let grown = r.age >= 15;
    let mut w: Vec<(&'static str, f32)> = Vec::new();
    // As the model's `is_female` does: `Feminine_Body` whole, and `Female_Breasts` by
    // `breasts_size` (1 by default, up to 2); the texture is swapped by the engine.
    if r.woman {
        w.push(("Feminine_Body", 1.0));
        if grown {
            w.push(("Female_Breasts", 0.6 + u(1) * 0.7));
            w.push(("Hips_Width", u(2) * 0.4));
        }
    } else if grown {
        w.push(("Shoulders_Width", 0.1 + u(3) * 0.5));
        w.push(("Jaw_Chin", 0.1 + u(4) * 0.6));
        w.push(("Traps_Neck", u(5) * 0.4));
    }
    match r.trade {
        // Strong in the arms, shoulders and neck. `Body_Muscle` and `Chest_Pecs` both push the
        // chest out where `Female_Breasts` does: past about 0.45 and 0.15 together, a man's
        // chest stands out further than a woman's (the shirt follows, with the same keys).
        Trade::Smith if grown && !r.woman => {
            w.extend([
                ("Body_Muscle", 0.35 + u(6) * 0.15),
                ("Arms_Upper", 0.7 + u(7) * 0.3),
                ("Forearms_Mass", 0.7 + u(8) * 0.3),
                ("Chest_Pecs", 0.05 + u(9) * 0.1),
                ("Torso_V_Taper", 0.3 + u(12) * 0.3),
                ("Traps_Neck", 0.3 + u(13) * 0.3),
            ]);
        }
        Trade::Tavern if grown => w.extend([("Body_Heavy_Fat", 0.5 + u(6) * 0.4), ("Abdomen_Belly", 0.4 + u(7) * 0.5)]),
        Trade::Carpenter | Trade::Sexton if grown && !r.woman => {
            w.extend([("Body_Muscle", 0.2 + u(6) * 0.25), ("Arms_Upper", 0.3 + u(7) * 0.3)])
        }
        Trade::Herder if grown => w.extend([("Body_Gaunt_Lean", 0.3 + u(6) * 0.3)]),
        Trade::Fisher if grown => w.extend([("Body_Gaunt_Lean", 0.2 + u(6) * 0.3), ("Arms_Upper", 0.2 + u(7) * 0.3)]),
        Trade::Chief if grown => w.extend([("Abdomen_Belly", 0.2 + u(6) * 0.3), ("Body_Muscle", 0.15 + u(7) * 0.25)]),
        _ if grown && u(10) < 0.3 => w.push(("Body_Heavy_Fat", u(6) * 0.45)),
        _ => {}
    }
    if r.age >= 55 {
        w.push(("Elder_Posture", ((r.age as f32 - 50.0) / 25.0).clamp(0.2, 1.0)));
        w.push(("Body_Gaunt_Lean", 0.3 + u(11) * 0.4));
    } else if !grown {
        w.push(("Body_Gaunt_Lean", 0.2));
    }
    // A little of everyone's own face and build.
    for (k, key) in
        ["Head_Shape", "Face_Cheeks_Brow", "Waist_Width", "Thighs_Mass", "Calves_Mass"].into_iter().enumerate()
    {
        w.push((key, u(20 + k as u64) * 0.5));
    }
    // No `_Height_*_Comp`: in the model they undo what `height_m` does to the bones, and here
    // the height is the entity's whole scale.
    // Several entries can name the same key (a lean fisher grown old): they add up.
    let mut merged: Vec<(&'static str, f32)> = Vec::new();
    for (key, weight) in w {
        match merged.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = (entry.1 + weight).min(if key == "Female_Breasts" { 2.0 } else { 1.0 }),
            None => merged.push((key, weight)),
        }
    }
    // A man's chest never stands out like a woman's, whatever made it: what the keys push the
    // bust out past `MALE_BUST` is taken back by `Female_Breasts` below 0, which moves just
    // that part of the chest (and of the shirt, which has the same keys).
    if !r.woman {
        let pushed: f32 = merged
            .iter()
            .filter_map(|&(key, weight)| BUST_PUSH.iter().find(|(k, _)| *k == key).map(|(_, cm)| cm * weight))
            .sum();
        if pushed > MALE_BUST {
            merged.push(("Female_Breasts", -(pushed - MALE_BUST) / BREASTS_PUSH));
        }
    }
    merged
}

/// How far (cm) each shape key at 1 pushes the front of the bust, measured on the chartest and
/// its shirts (the larger of the two).
const BUST_PUSH: [(&str, f32); 4] =
    [("Chest_Pecs", 4.2), ("Body_Muscle", 3.8), ("Body_Heavy_Fat", 2.5), ("_Height_Short_Comp", 1.5)];
/// How far `Female_Breasts` at 1 pushes it.
const BREASTS_PUSH: f32 = 5.3;
/// The most a man's bust may stand out from the bare model's, cm.
const MALE_BUST: f32 = 0.8;

/// Cloth dyed as in the 17th and 18th centuries, worn and faded: undyed linen, ochre, madder,
/// woad, walnut, weld and ash, sRGB. Shirts take the light ones, the rest anything.
const LINEN: [[f32; 3]; 4] = [[0.92, 0.89, 0.80], [0.86, 0.82, 0.70], [0.80, 0.78, 0.72], [0.88, 0.84, 0.76]];
const DYES: [[f32; 3]; 9] = [
    [0.78, 0.64, 0.42],
    [0.66, 0.44, 0.36],
    [0.50, 0.56, 0.64],
    [0.55, 0.43, 0.32],
    [0.64, 0.64, 0.44],
    [0.60, 0.58, 0.54],
    [0.46, 0.54, 0.50],
    [0.58, 0.36, 0.30],
    [0.44, 0.38, 0.34],
];

/// What `r` wears: files in the chartest's `clothes/` and their colors, sRGB. A top and a
/// bottom, or a long tunic alone; women a blouse and a skirt (now and then trousers), men a
/// shirt and trousers, fishers knee breeches and the chief a tunic.
pub fn clothes(r: &Resident) -> Vec<(&'static str, [f32; 3])> {
    let mut worn = dressed(r);
    worn.extend(accessories(r));
    worn
}

fn dressed(r: &Resident) -> Vec<(&'static str, [f32; 3])> {
    let id = r.id.0;
    let u = |k: u64| unit(id, 400 + k);
    let pick = |list: &[[f32; 3]], k: u64| list[(u(k) * list.len() as f64) as usize % list.len()];
    let top_color = if u(1) < 0.65 { pick(&LINEN, 2) } else { pick(&DYES, 2) };
    let bottom_color = pick(&DYES, 3);
    let grown = r.age >= 15;
    if r.trade == Trade::Chief && r.kin == Kin::Head || !r.woman && r.age >= 60 && u(0) < 0.5 {
        return vec![("clothes/TunicaLonga", pick(&DYES, 4))];
    }
    if r.woman {
        let bottom = if grown && u(5) < 0.2 { "clothes/CalcaLinho" } else { "clothes/SaiaLonga" };
        return vec![("clothes/BlusaFeminina", top_color), (bottom, bottom_color)];
    }
    let short_sleeves = matches!(r.trade, Trade::Smith | Trade::Fisher) && r.kin != Kin::Elder || u(6) < 0.3;
    let top = if short_sleeves { "clothes/CamisaMangaCurtaBranca" } else { "clothes/CamisaLinhoClara" };
    let breeches = r.trade == Trade::Fisher || !grown && u(7) < 0.6;
    vec![(top, top_color), (if breeches { "clothes/CalcaCurtaJoelho" } else { "clothes/CalcaLinho" }, bottom_color)]
}

/// Aprons and the like, from `accessories/`: leather for the smith, cloth at the tavern and now
/// and then for the healer or whoever cooks at home, and a holster for the chief.
fn accessories(r: &Resident) -> Vec<(&'static str, [f32; 3])> {
    let u = |k: u64| unit(r.id.0, 450 + k);
    let grown = r.age >= 15;
    let working = matches!(r.kin, Kin::Head | Kin::Spouse | Kin::Sibling);
    let mut worn = Vec::new();
    if !grown {
        return worn;
    }
    match r.trade {
        Trade::Smith if working => worn.push(("accessories/AventalCouroFerreiro", [1.0, 1.0, 1.0])),
        Trade::Tavern if working => worn.push(("accessories/AventalPanoTaverna", LINEN[(u(0) * 4.0) as usize % 4])),
        Trade::Healer if u(1) < 0.5 => worn.push(("accessories/AventalPanoTaverna", DYES[(u(0) * 9.0) as usize % 9])),
        Trade::Home if r.woman && working && u(1) < 0.4 => {
            worn.push(("accessories/AventalPanoTaverna", LINEN[(u(0) * 4.0) as usize % 4]))
        }
        Trade::Chief if r.kin == Kin::Head => worn.push(("accessories/Coldre_Couro", [1.0, 1.0, 1.0])),
        _ => {}
    }
    worn
}

/// A hat `r` wears on the head (a file name under the model's folder) and its tint, sRGB:
/// straw for those who work in the sun, field and boat. Rigid on the `head` joint, like hair;
/// the hat is fitted to the bare head, and hair under it is for the engine to hide.
pub fn hat(r: &Resident) -> Option<(&'static str, [f32; 3])> {
    let u = |k: u64| unit(r.id.0, 500 + k);
    if r.age < 12 {
        return None;
    }
    let chance = match r.trade {
        Trade::Fisher => 0.85,
        Trade::Herder => 0.8,
        Trade::Home if !r.woman => 0.7,
        Trade::Home => 0.45,
        Trade::Chief => 0.0,
        _ => 0.15,
    };
    if u(0) >= chance {
        return None;
    }
    const STRAW: [[f32; 3]; 3] = [[1.0, 1.0, 1.0], [0.92, 0.86, 0.74], [0.8, 0.74, 0.62]];
    Some(("accessories/ChapeuPalha", STRAW[(u(1) * 3.0) as usize % 3]))
}

/// Which of the chartest's hairs `r` wears (a file name in `hairs/`) and its color, sRGB;
/// `None`: bald. Grey with age; never a bun under a hat.
pub fn hair(r: &Resident) -> Option<(&'static str, [f32; 3])> {
    let id = r.id.0;
    let roll = unit(id, 300);
    let style = if r.woman {
        if roll < 0.55 { "CoqueSoltoFeminino" } else { "CabeloLongoSolto" }
    } else {
        let bald = if r.age >= 45 { 0.4 } else { 0.12 };
        if roll < bald {
            return None;
        }
        if roll < bald + (1.0 - bald) * 0.6 {
            "CabeloCurtoMasculino"
        } else if roll < 0.92 {
            "CoqueSamuraiMasculino"
        } else {
            "CabeloLongoSolto"
        }
    };
    // A bun pokes through a hat's crown: under one, short hair for men and loose for women.
    let style = match style {
        "CoqueSamuraiMasculino" if hat(r).is_some() => "CabeloCurtoMasculino",
        "CoqueSoltoFeminino" if hat(r).is_some() => "CabeloLongoSolto",
        style => style,
    };
    const COLORS: [[f32; 3]; 6] = [
        [0.12, 0.09, 0.07],
        [0.22, 0.14, 0.08],
        [0.35, 0.22, 0.12],
        [0.55, 0.38, 0.2],
        [0.48, 0.2, 0.1],
        [0.08, 0.07, 0.07],
    ];
    let mut color = COLORS[(path_seed(&[id, 301]) % COLORS.len() as u64) as usize];
    if r.age >= 50 {
        let grey = ((r.age as f32 - 45.0) / 25.0).clamp(0.0, 0.9);
        color = color.map(|c| c + (0.7 - c) * grey);
    }
    Some((style, color))
}
