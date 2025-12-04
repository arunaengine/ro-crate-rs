use std::path::PathBuf;

use rocraters::ro_crate::constraints::EntityValue;
use rocraters::ro_crate::write::is_not_url;

use crate::error::TuiError;
use crate::user_input::App;

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

                self.subs = rocrate
                    .get_subcrates()
                    .iter()
                    .map(|e| e.get_id().clone())
                    .collect();
                self.ids = rocrate
                    .get_all_ids()
                    .iter()
                    .map(|id| id.to_string())
                    .collect();
                self.state.push((path.to_string(), rocrate));
                self.view = "Loaded crate ...".to_string();
                self.cursor = self.state.len() - 1;
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

                        let (subdir, subcrate) = if is_not_url(subcrate_id) {
                            if let Some((_, parent)) = self.state.get(self.cursor) {
                                let (_, subcrate_path) = parent
                                    .get_entity(subcrate_id)
                                    .ok_or_else(|| TuiError::IdNotFound(subcrate_id.to_string()))?
                                    .get_specific_property("subjectOf")
                                    .ok_or_else(|| {
                                        TuiError::PropertyNotFound("subjectOf".to_string())
                                    })?;

                                let EntityValue::EntityId(
                                    rocraters::ro_crate::constraints::Id::Id(subcrate_path),
                                ) = subcrate_path
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
                                    .ok_or_else(|| {
                                        TuiError::PropertyNotFound("subjectOf".to_string())
                                    })?;
                                let EntityValue::EntityId(
                                    rocraters::ro_crate::constraints::Id::Id(subcrate_url),
                                ) = subcrate_url
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

                        self.subs = subcrate
                            .get_subcrates()
                            .iter()
                            .map(|e| e.get_id().clone())
                            .collect();
                        self.ids = subcrate
                            .get_all_ids()
                            .iter()
                            .map(|id| id.to_string())
                            .collect();
                        self.state.push((subdir, subcrate));
                        //self.view = format!("Loaded subcrate {subcrate_id}");
                        self.cursor = self.state.len() - 1;
                    }
                }
            }
            ("pwd", _) => {
                self.view = match self.state.get(self.cursor) {
                    Some((p, _)) => p.to_string(),
                    None => return Err(TuiError::NoState),
                };
            }
            ("help", _) => {
                self.view = format!(
                    "Commands:
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
  - DOIs resolving to Zenodo/similar repositories"
                );
            }
            (cmd, arg) => {
                self.view = format!(
                    "Unkown command: {cmd} {arg}

Commands:
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
  - DOIs resolving to Zenodo/similar repositories"
                );
            }
        };

        Ok(())
    }
}
