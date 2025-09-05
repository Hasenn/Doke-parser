use polib::{catalog::Catalog, message::{Message, MessageBuilder}, metadata::CatalogMetadata, po_file::{self, POParseError}};
use std::{collections::HashMap, path::Path};

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