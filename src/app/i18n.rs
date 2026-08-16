use std::collections::HashMap;

pub const LANG_EN: &str = "en";
pub const LANG_ZH: &str = "zh";
pub const DEFAULT_LANG: &str = LANG_EN;

pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
];

#[derive(Default, Clone)]
pub struct Localizer {
    table: HashMap<String, String>,
    pub language: String,
}

impl Localizer {
    pub fn new(language: &str) -> Self {
        let mut this = Localizer {
            table: HashMap::new(),
            language: language.to_string(),
        };
        this.reload(language);
        this
    }

    pub fn set_language(&mut self, language: &str) {
        self.reload(language);
    }

    pub fn reload(&mut self, language: &str) {
        self.language = language.to_string();
        match language {
            LANG_ZH => self.table = load_table(include_str!("../assets/zh.json")),
            _ => self.table = load_table(include_str!("../assets/en.json")),
        }
    }

    pub fn t(&self, key: &str, args: &[&str]) -> String {
        let tmpl = self
            .table
            .get(key)
            .map(|s| s.as_str())
            .unwrap_or(key);
        if args.is_empty() {
            return tmpl.to_string();
        }
        let mut out = String::with_capacity(tmpl.len() + 32);
        let mut chars = tmpl.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut idx = String::new();
                for p in chars.by_ref() {
                    if p == '}' {
                        break;
                    }
                    idx.push(p);
                }
                if let Ok(n) = idx.parse::<usize>() {
                    if let Some(arg) = args.get(n) {
                        out.push_str(arg);
                        continue;
                    }
                }
                out.push_str(&format!("{{{idx}}}"));
            } else {
                out.push(c);
            }
        }
        out
    }
}

fn load_table(json: &str) -> HashMap<String, String> {
    serde_json::from_str::<HashMap<String, String>>(json)
        .unwrap_or_default()
}

