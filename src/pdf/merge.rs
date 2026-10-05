//! Merging several PDF files into one.

use anyhow::{bail, Context, Result};
use lopdf::{dictionary, Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

fn is_type(obj: &Object, ty: &[u8]) -> bool {
    match obj {
        Object::Dictionary(d) => d.get(b"Type").and_then(|t| t.as_name()).map(|n| n == ty).unwrap_or(false),
        _ => false,
    }
}

/// Copies attributes that pages may inherit from their parents onto the page itself,
/// because the original page tree is discarded.
fn flatten_inherited(doc: &mut Document, page_id: ObjectId) {
    for key in INHERITABLE {
        let has = doc
            .get_object(page_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .map(|d| d.has(key))
            .unwrap_or(true);
        if has {
            continue;
        }
        let mut parent = doc
            .get_object(page_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Parent").ok())
            .and_then(|p| p.as_reference().ok());
        let mut found: Option<Object> = None;
        let mut guard = 0;
        while let Some(pid) = parent {
            guard += 1;
            if guard > 64 {
                break;
            }
            let dict = match doc.get_object(pid).ok().and_then(|o| o.as_dict().ok()) {
                Some(d) => d,
                None => break,
            };
            if let Ok(v) = dict.get(key) {
                found = Some(v.clone());
                break;
            }
            parent = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
        }
        if let Some(value) = found {
            if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
                d.set(key.to_vec(), value);
            }
        }
    }
}

pub fn merge_pdfs(inputs: &[PathBuf], out: &Path) -> Result<()> {
    if inputs.len() < 2 {
        bail!("select at least two PDF files to merge");
    }
    let mut max_id = 1u32;
    let mut all_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut all_pages: Vec<ObjectId> = Vec::new();

    for path in inputs {
        let mut doc = Document::load(path).with_context(|| format!("cannot open {}", path.display()))?;
        if doc.trailer.get(b"Encrypt").is_ok() {
            bail!("{} is password protected", path.display());
        }
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        for &page in &pages {
            flatten_inherited(&mut doc, page);
        }
        all_pages.extend(pages);

        for (id, obj) in doc.objects.into_iter() {
            if is_type(&obj, b"Catalog") || is_type(&obj, b"Pages") || is_type(&obj, b"Outlines") {
                continue;
            }
            all_objects.insert(id, obj);
        }
    }

    let mut merged = Document::with_version("1.7");
    merged.objects = all_objects;
    merged.max_id = max_id;
    let pages_id = merged.new_object_id();

    for page in &all_pages {
        if let Ok(Object::Dictionary(d)) = merged.get_object_mut(*page) {
            d.set("Parent", Object::Reference(pages_id));
        }
    }
    let kids: Vec<Object> = all_pages.iter().map(|id| Object::Reference(*id)).collect();
    let count = kids.len() as i64;
    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => Object::Integer(count),
        }),
    );
    let catalog_id = merged.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    merged.trailer.set("Root", Object::Reference(catalog_id));
    merged.prune_objects();
    merged.compress();
    merged.save(out).with_context(|| format!("cannot write {}", out.display()))?;
    Ok(())
}
