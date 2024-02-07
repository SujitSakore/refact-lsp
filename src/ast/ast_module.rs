use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::fs::read_to_string;
use tokio::sync::Mutex as AMutex;
use tokio::task::JoinHandle;
use tower_lsp::lsp_types::WorkspaceFolder;
use tracing::info;
use tree_sitter::Point;

use crate::ast::ast_index::AstIndex;
use crate::ast::ast_index_service::AstIndexService;
use crate::ast::ast_search_engine::AstSearchEngine;
use crate::ast::structs::{AstCursorSearchResult, AstQuerySearchResult};
use crate::global_context::CommandLine;
use crate::vecdb::file_filter;

pub struct AstModule {
    ast_index_service: Arc<AMutex<AstIndexService>>,
    ast_index: Arc<AMutex<AstIndex>>,
    ast_search_engine: Arc<AMutex<AstSearchEngine>>,
    cmdline: CommandLine,
}

#[derive(Debug, Serialize)]
pub struct VecDbCaps {
    functions: Vec<String>,
}


impl AstModule {
    pub async fn init(
        cmdline: CommandLine,
    ) -> Result<AstModule, String> {
        let ast_index = Arc::new(AMutex::new(AstIndex::init()));
        let ast_search_engine = Arc::new(AMutex::new(AstSearchEngine::init(ast_index.clone())));
        let ast_index_service = Arc::new(AMutex::new(AstIndexService::init(ast_index.clone())));
        Ok(AstModule {
            ast_index_service,
            ast_index,
            ast_search_engine,
            cmdline,
        })
    }

    pub async fn start_background_tasks(&self) -> Vec<JoinHandle<()>> {
        info!("ast module: start_background_tasks");
        return self.ast_index_service.lock().await.start_background_tasks().await;
    }

    pub async fn add_or_update_file(&self, file_path: PathBuf, force: bool) {
        self.ast_index_service.lock().await.process_file(file_path, force).await;
    }

    pub async fn add_or_update_files(&self, file_paths: Vec<PathBuf>, force: bool) {
        self.ast_index_service.lock().await.process_files(file_paths, force).await;
    }

    pub async fn init_folders(&self, folders: Vec<WorkspaceFolder>) {
        let files = file_filter::retrieve_files_by_proj_folders(
            folders.iter().map(|x| PathBuf::from(x.uri.path())).collect()
        ).await;
        self.add_or_update_files(files, true).await;
        info!("ast: init_folders complete");
    }

    pub async fn remove_file(&self, file_path: &PathBuf) {
        let _ = self.ast_index.lock().await.remove(file_path).await;
    }

    pub async fn search_by_cursor(
        &self,
        file_path: &PathBuf,
        cursor: Point
    ) -> Result<AstCursorSearchResult, String> {
        let t0 = std::time::Instant::now();

        let text = match read_to_string(file_path).await {
            Ok(s) => s,
            Err(e) => return Err(e.to_string())
        };

        let mut handler_locked = self.ast_search_engine.lock().await;
        let results = match handler_locked.search(
            file_path,
            text.as_str(),
            cursor,
        ).await {
            Ok(res) => res,
            Err(_) => { return Err("error during search occurred".to_string()); }
        };
        for rec in results.iter() {
            info!("distance {:.3}, found ...{}, ", rec.lev_dist_to_query, rec.symbol_declaration.meta_path);
        }
        Ok(
            AstCursorSearchResult {
                query_text: text.to_string(),
                file_path: file_path.clone(),
                cursor: cursor,
                search_results: results,
            }
        )
    }

    pub async fn search_by_symbol_path(
        &mut self,
        symbol_path: String,
        top_n: usize
    ) -> Result<AstQuerySearchResult, String> {
        let t0 = std::time::Instant::now();
        let ast_index = self.ast_index.clone();
        let ast_index_locked  = ast_index.lock().await;
        match ast_index_locked.search(symbol_path.as_str(), top_n, None).await {
            Ok(results) =>
                Ok(
                    AstQuerySearchResult {
                        query_text: symbol_path,
                        search_results: results,
                    }
                ),
            Err(e) => Err(e.to_string())
        }
    }
}
