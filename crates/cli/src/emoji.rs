/// planet icon(animal key)→ emoji;CLI 与 GUI 图标并存,同一字段两种呈现
const FALLBACK: &str = "🪐";

pub fn icon_to_emoji(icon: &str) -> &'static str {
    let key = icon.strip_prefix("animal.").unwrap_or(icon);
    EMOJI
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, e)| *e)
        .unwrap_or(FALLBACK)
}

static EMOJI: &[(&str, &str)] = &[
    ("cat", "🐱"), ("dog", "🐶"), ("panda", "🐼"), ("fox", "🦊"), ("bear", "🐻"),
    ("rabbit", "🐰"), ("tiger", "🐯"), ("lion", "🦁"), ("pig", "🐷"), ("cow", "🐮"),
    ("chicken", "🐔"), ("duck", "🦆"), ("penguin", "🐧"), ("owl", "🦉"), ("frog", "🐸"),
    ("monkey", "🐵"), ("koala", "🐨"), ("horse", "🐴"), ("sheep", "🐑"), ("goat", "🐐"),
    ("mouse", "🐭"), ("hamster", "🐹"), ("squirrel", "🐿️"), ("hedgehog", "🦔"), ("bat", "🦇"),
    ("wolf", "🐺"), ("deer", "🦌"), ("elephant", "🐘"), ("rhino", "🦏"), ("hippo", "🦛"),
    ("crocodile", "🐊"), ("turtle", "🐢"), ("snake", "🐍"), ("octopus", "🐙"), ("crab", "🦀"),
    ("whale", "🐳"), ("dolphin", "🐬"), ("shark", "🦈"), ("eagle", "🦅"), ("parrot", "🦜"),
];
