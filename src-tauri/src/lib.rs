use lopdf::{Document, Object, ObjectId};
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct PdfOperationResult {
    ok: bool,
    message: String,
    file_count: usize,
}

#[derive(Serialize)]
struct PdfPageCountResult {
    ok: bool,
    message: String,
    page_count: usize,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedSplitPage {
    page_number: u32,
    rotation: i32,
}

fn normalize_rotation(rotation: i32) -> i32 {
    ((rotation % 360) + 360) % 360
}

#[tauri::command]
fn get_pdf_page_count(input_path: String) -> Result<PdfPageCountResult, String> {
    let doc = Document::load(&input_path)
        .map_err(|err| format!("Failed to load PDF '{}': {}", input_path, err))?;

    let page_count = doc.get_pages().len();

    if page_count == 0 {
        return Err("The input PDF has no pages".to_string());
    }

    Ok(PdfPageCountResult {
        ok: true,
        message: format!("Loaded '{}' with {} pages", input_path, page_count),
        page_count,
    })
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
            "PDF fusionné: {} PDFs vers '{}'",
            input_paths.len(),
            output_path
        ),
        file_count: input_paths.len(),
    })
}

#[tauri::command]
fn split_pdf(
    input_path: String,
    selected_pages: Vec<SelectedSplitPage>,
    output_path: String,
) -> Result<PdfOperationResult, String> {
    if selected_pages.is_empty() {
        return Err("No pages selected".to_string());
    }

    let mut cleaned_pages: Vec<SelectedSplitPage> = Vec::new();

    for page in selected_pages {
        if !cleaned_pages
            .iter()
            .any(|existing| existing.page_number == page.page_number)
        {
            cleaned_pages.push(SelectedSplitPage {
                page_number: page.page_number,
                rotation: normalize_rotation(page.rotation),
            });
        }
    }

    let source = Document::load(&input_path)
        .map_err(|err| format!("Failed to load PDF '{}': {}", input_path, err))?;

    let page_map = source.get_pages();
    let page_numbers: Vec<u32> = page_map.keys().copied().collect();
    if page_numbers.is_empty() {
        return Err("The input PDF has no pages".to_string());
    }

    for page in &cleaned_pages {
        if !page_numbers.contains(&page.page_number) {
            return Err(format!(
                "Selected page {} does not exist in the PDF",
                page.page_number
            ));
        }
    }

    let selected_page_ids: Vec<ObjectId> = cleaned_pages
        .iter()
        .map(|page| {
            page_map
                .get(&page.page_number)
                .copied()
                .ok_or_else(|| {
                    format!("Selected page {} does not exist in the PDF", page.page_number)
                })
        })
        .collect::<Result<_, _>>()?;

    let mut split_doc = source.clone();

    for page in &cleaned_pages {
        let page_id = page_map
            .get(&page.page_number)
            .copied()
            .ok_or_else(|| format!("Selected page {} does not exist in the PDF", page.page_number))?;

        let page_dictionary = split_doc
            .get_dictionary_mut(page_id)
            .map_err(|err| format!("Failed to update page {}: {}", page.page_number, err))?;

        page_dictionary.set("Rotate", page.rotation as i64);
    }

    let pages_object_id = split_doc
        .catalog()
        .map_err(|err| format!("Failed to read PDF catalog: {}", err))?
        .get(b"Pages")
        .map_err(|err| format!("Failed to read Pages root: {}", err))?
        .as_reference()
        .map_err(|err| format!("Pages root is not a reference: {}", err))?;

    {
        let pages_dictionary = split_doc
            .get_dictionary_mut(pages_object_id)
            .map_err(|err| format!("Failed to update Pages root: {}", err))?;

        pages_dictionary.set("Count", selected_page_ids.len() as u32);
        pages_dictionary.set(
            "Kids",
            selected_page_ids
                .into_iter()
                .map(Object::Reference)
                .collect::<Vec<_>>(),
        );
    }

    split_doc.prune_objects();
    split_doc.renumber_objects();
    split_doc.compress();

    split_doc
        .save(&output_path)
        .map_err(|err| format!("Failed to save split PDF '{}': {}", output_path, err))?;

    Ok(PdfOperationResult {
        ok: true,
        message: format!(
            "PDF modifié: {} pages de '{}' vers '{}'",
            cleaned_pages.len(),
            input_path,
            output_path
        ),
        file_count: 1,
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
        .invoke_handler(tauri::generate_handler![merge_pdfs, split_pdf, get_pdf_page_count])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
