use std::path::PathBuf;

use rocrate_indexer::SearchHit;
use rocraters::ro_crate::constraints::EntityValue;
use rocraters::ro_crate::rocrate::RoCrate;
use rocraters::ro_crate::write::is_not_url;
use serde::Serialize;

use crate::error::TuiError;
use crate::user_input::App;

const HELP_MESSAGE: &'static str = "Commands:
  load <url|path>    Load RO-Crate from URL or .zip file
  ls                 Lists the full RO-Crate
  ls sub             List all subcrates
  ls ids             List all ids used in the RO-Crate
  ls props           List all used properties in the RO-Crate
  get <@id>          Pretty-print JSON for entity
  cd <id>            Enter subcrate or folder
  cd ..              Return to parent
  cd /               Return to root crate
  pwd                Print current path
  help               Show this message
Supported formats:
  - .zip archives containing ro-crate-metadata.json
  - Direct URLs to RO-Crate archives
  - DOIs resolving to Zenodo/similar repositories";

impl App {
    pub fn handle_commands(&mut self) -> Result<(), TuiError> {
        let (cmd, arg) = self.input.split_once(' ').unwrap_or((&self.input, ""));
        match (cmd, arg) {
            ("load", path) => {
                let rocrate = if is_not_url(path) {
                    let crate_path = if path.ends_with('/') {
                        PathBuf::from(format!("{}ro-crate-metadata.json", path))
                    } else {
                        PathBuf::from(format!("{}/ro-crate-metadata.json", path))
                    };
                    rocraters::ro_crate::read::read_crate(&crate_path, 0)
                } else {
                    let url = url::Url::parse(path)?;
                    rocraters::ro_crate::read::load_remote(url, 0)
                }?;

                self.import_crate_state(path.to_string(), rocrate)?;
            }
            ("ls", option) => {
                if let Some(rocrate) = self.state.get(self.cursor) {
                    match option {
                        "sub" => {
                            let subcrates = rocrate.1.get_subcrates();
                            self.view = serde_json::to_string_pretty(&subcrates)?;
                        }
                        "props" => {
                            let props = rocrate.1.get_all_properties();
                            self.view = serde_json::to_string_pretty(&props)?;
                        }
                        "ids" => {
                            let ids = rocrate.1.get_all_ids();
                            self.view = serde_json::to_string_pretty(&ids)?;
                        }
                        "" => {
                            self.view = serde_json::to_string_pretty(&rocrate.1)?;
                        }
                        _ => {
                            return Err(TuiError::CommandNotFound(option.to_string()));
                        }
                    }
                } else {
                    return Err(TuiError::NoState);
                }
            }
            ("get", id) => match self.state.get(self.cursor) {
                Some((_, rocrate)) => match rocrate.get_entity(id) {
                    Some(entity) => {
                        self.view = serde_json::to_string_pretty(entity)?;
                    }
                    None => {
                        return Err(TuiError::IdNotFound(id.to_string()));
                    }
                },
                None => {
                    return Err(TuiError::NoState);
                }
            },
            ("cd", subcrate_id) => {
                match subcrate_id {
                    ".." => {
                        self.view = format!("Changed to upper level");
                        self.cursor -= 1;
                    }
                    "/" => {
                        self.view = format!("Changed to root level");
                        self.cursor = 0;
                    }
                    _ => {
                        // TODO: Improve!
                        // This is only a hacky way of checking if subcrate was already loaded
                        if let Some(already_loaded_idx) =
                            self.state.iter().enumerate().find_map(|(idx, (p, _))| {
                                if p.contains(subcrate_id) {
                                    Some(idx)
                                } else {
                                    None
                                }
                            })
                        {
                            self.view = format!("Loaded subcrate {subcrate_id}");
                            self.cursor = already_loaded_idx;
                            return Ok(());
                        }

                        let (subdir, subcrate) = self.get_subcrate_with_path(subcrate_id)?;

                        self.import_crate_state(subdir, subcrate)?;
                    }
                }
            }
            ("pwd", _) => {
                self.view = match self.state.get(self.cursor) {
                    Some((p, _)) => p.to_string(),
                    None => return Err(TuiError::NoState),
                };
            }
            ("search", query) => {
                // TODO: Improve showing search results
                let mut results = vec![];
                for SearchHit {
                    entity_id,
                    crate_id,
                    ..
                } in self.idxer.search(query, 100)?
                {
                    if let Some(rocrate) = self.idxer.get_crate(&crate_id) {
                        if let Some(hit) = rocrate.get_entity(&entity_id) {
                            results.push((crate_id, hit));
                        }
                    }
                }
                self.view = serde_json::to_string_pretty(&results)?;
            }
            ("help", _) => {
                self.view = HELP_MESSAGE.to_string();
            }
            (cmd, arg) => {
                self.view = format!(
                    "Unkown command: {cmd} {arg}

{}",
                    HELP_MESSAGE
                );
            }
        };

        Ok(())
    }

    pub fn import_crate_state(&mut self, path: String, rocrate: RoCrate) -> Result<(), TuiError> {
        let _ = self
            .idxer
            .add_from_json(serde_json::to_string(&rocrate)?.as_str(), Some(&path))?;
        self.completion.subs = rocrate
            .get_subcrates()
            .iter()
            .map(|e| e.get_id().clone())
            .collect();
        self.completion.ids = rocrate
            .get_all_ids()
            .iter()
            .map(|id| id.to_string())
            .collect();
        self.state.push((path, rocrate));
        self.view = "Loaded crate ...".to_string();
        self.cursor = self.state.len() - 1;
        Ok(())
    }

    pub fn get_subcrate_with_path(&self, subcrate_id: &str) -> Result<(String, RoCrate), TuiError> {
        let (subdir, subcrate) = if is_not_url(subcrate_id) {
            if let Some((_, parent)) = self.state.get(self.cursor) {
                let (_, subcrate_path) = parent
                    .get_entity(subcrate_id)
                    .ok_or_else(|| TuiError::IdNotFound(subcrate_id.to_string()))?
                    .get_specific_property("subjectOf")
                    .ok_or_else(|| TuiError::PropertyNotFound("subjectOf".to_string()))?;

                let EntityValue::EntityId(rocraters::ro_crate::constraints::Id::Id(subcrate_path)) =
                    subcrate_path
                else {
                    return Err(TuiError::PropertyNotFound("@id".to_string()));
                };
                (
                    subcrate_id.to_string(),
                    rocraters::ro_crate::read::read_crate(
                        &PathBuf::from(&subcrate_path.to_string()),
                        0,
                    )?,
                )
            } else {
                return Err(TuiError::IdNotFound(subcrate_id.to_string()));
            }
        } else {
            if let Some((_, parent)) = self.state.get(self.cursor) {
                let (_, subcrate_url) = parent
                    .get_entity(subcrate_id)
                    .ok_or_else(|| TuiError::IdNotFound(subcrate_id.to_string()))?
                    .get_specific_property("subjectOf")
                    .ok_or_else(|| TuiError::PropertyNotFound("subjectOf".to_string()))?;
                let EntityValue::EntityId(rocraters::ro_crate::constraints::Id::Id(subcrate_url)) =
                    subcrate_url
                else {
                    return Err(TuiError::PropertyNotFound("@id".to_string()));
                };
                let url = url::Url::parse(&subcrate_url.to_string())?;
                (
                    url.to_string(),
                    rocraters::ro_crate::read::load_remote(url, 0)?,
                )
            } else {
                return Err(TuiError::IdNotFound(subcrate_id.to_string()));
            }
        };
        Ok((subdir, subcrate))
    }
}
