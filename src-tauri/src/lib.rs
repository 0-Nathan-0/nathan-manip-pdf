use lopdf::{Document, Object, ObjectId};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct PdfOperationResult {
    ok: bool,
    message: String,
    file_count: usize,
}

#[tauri::command]
fn merge_pdfs(
    input_paths: Vec<String>,
    output_path: String,
) -> Result<PdfOperationResult, String> {
    if input_paths.len() < 2 {
        return Err("At least 2 input files are required".to_string());
    }

    let documents: Vec<Document> = input_paths
        .iter()
        .map(|path| {
            Document::load(path)
                .map_err(|err| format!("Failed to load PDF '{}': {}", path, err))
        })
        .collect::<Result<_, _>>()?;

    let mut max_id = 1;
    let mut documents_pages = BTreeMap::<ObjectId, Object>::new();
    let mut documents_objects = BTreeMap::<ObjectId, Object>::new();
    let mut document = Document::with_version("1.5");

    for mut source_document in documents {
        source_document.renumber_objects_with(max_id);
        max_id = source_document.max_id + 1;

        let page_objects = source_document
            .get_pages()
            .into_iter()
            .try_fold(BTreeMap::new(), |mut acc, (_, object_id)| {
                let object = source_document
                    .get_object(object_id)
                    .map_err(|err| format!("Failed to read a page object: {}", err))?
                    .to_owned();

                acc.insert(object_id, object);
                Ok::<_, String>(acc)
            })?;

        documents_pages.extend(page_objects);
        documents_objects.extend(source_document.objects);
    }

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                if catalog_object.is_none() {
                    catalog_object = Some((*object_id, object.clone()));
                }
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();

                    if let Some((_, ref existing_pages)) = pages_object {
                        if let Ok(old_dictionary) = existing_pages.as_dict() {
                            dictionary.extend(old_dictionary);
                        }
                    }

                    let pages_id = pages_object
                        .as_ref()
                        .map(|(existing_id, _)| *existing_id)
                        .unwrap_or(*object_id);

                    pages_object = Some((pages_id, Object::Dictionary(dictionary)));
                }
            }
            b"Page" => {}
            b"Outlines" => {}
            b"Outline" => {}
            _ => {
                document.objects.insert(*object_id, object.clone());
            }
        }
    }

    let catalog_object = catalog_object.ok_or_else(|| "Catalog root not found".to_string())?;
    let pages_object = pages_object.ok_or_else(|| "Pages root not found".to_string())?;

    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_object.0);

            document
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    if let Ok(dictionary) = pages_object.1.as_dict() {
        let mut dictionary = dictionary.clone();

        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages
                .into_iter()
                .map(|(object_id, _)| Object::Reference(object_id))
                .collect::<Vec<_>>(),
        );

        document
            .objects
            .insert(pages_object.0, Object::Dictionary(dictionary));
    }

    if let Ok(dictionary) = catalog_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_object.0);
        dictionary.remove(b"Outlines");

        document
            .objects
            .insert(catalog_object.0, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_object.0);
    document.max_id = document.objects.len() as u32;
    document.renumber_objects();
    document.adjust_zero_pages();
    document.compress();

    document
        .save(&output_path)
        .map_err(|err| format!("Failed to save merged PDF to '{}': {}", output_path, err))?;

    Ok(PdfOperationResult {
        ok: true,
        message: format!(
            "Successfully merged {} PDFs into {}",
            input_paths.len(),
            output_path
        ),
        file_count: input_paths.len(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![merge_pdfs])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
