use polib::{catalog::Catalog, message::{Message, MessageBuilder}, metadata::CatalogMetadata, po_file::{self, POParseError}};
use std::{collections::HashMap, hash::DefaultHasher, path::Path};
use std::hash::{Hash, Hasher};

pub fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
pub fn camel_to_const_case(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();
    let mut prev_was_upper = false;
    let mut prev_was_lower = false;
    
    while let Some(c) = chars.next() {
        let is_upper = c.is_uppercase();
        
        // Add underscore if:
        // 1. Current char is uppercase AND previous was lowercase (camelCase boundary)
        // 2. Current char is lowercase AND previous was uppercase AND next is uppercase (aBc -> A_BC)
        if !result.is_empty() {
            if is_upper && prev_was_lower {
                result.push('_');
            } else if let Some(&next) = chars.peek() {
                if !is_upper && prev_was_upper && next.is_uppercase() {
                    result.push('_');
                }
            }
        }
        
        result.push(c.to_ascii_uppercase());
        
        prev_was_upper = is_upper;
        prev_was_lower = !is_upper;
    }
    
    result
}

const BASE32_ALPHABET: [char; 32] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    '2', '3', '4', '5', '6', '7',
];

pub fn u64_to_base32(mut num: u64) -> String {
    if num == 0 {
        return "A".to_string();
    }
    
    let mut result = String::new();
    
    while num > 0 {
        let remainder = (num % 32) as usize;
        result.push(BASE32_ALPHABET[remainder]);
        num /= 32;
    }
    
    result.chars().rev().collect()
}

pub fn update_po_file(
    po_path: &Path,
    translations: HashMap<String, String>,
    project_id_version : String,
) -> Result<(), POParseError> {
    // Load existing PO file or create new
    let mut catalog = if po_path.exists() {
        po_file::parse(po_path)?
    } else {
        let mut meta = CatalogMetadata::new();
        meta.project_id_version = project_id_version;
        meta.language = "en".into();

        Catalog::new(meta)
    };
    for (msgid, msgentrad) in translations {
        let m_singular = Message::build_singular().with_msgid(msgid.clone()).with_msgstr(msgentrad.clone()).done();
        let m_plural = Message::build_plural()
            .with_msgid(format!("{}_PL", msgid.clone()))
            .with_msgstr(msgentrad.clone()).done();
        catalog.append_or_update(m_singular);
        catalog.append_or_update(m_plural);
    }

    // Save updated PO file
    po_file::write(&catalog, po_path)?;
    
    Ok(())
}