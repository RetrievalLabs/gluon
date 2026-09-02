use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use crate::languages::business::model::{CodeModel, InvocationInfo, MethodInfo, RelationshipInfo};
use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::business::modules::module_id_for_file;

pub struct JdtlsOptions {
    pub command: String,
    pub workspace: PathBuf,
    pub max_in_flight: usize,
}

#[derive(Debug, Clone)]
pub struct JdtlsDefinitionRequest {
    pub owner_id: String,
    pub file: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct JdtlsDefinition {
    pub owner_id: String,
    pub name: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct JdtlsSymbolRequest {
    pub file: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct JdtlsResolvedSymbol {
    pub file: String,
    pub line: usize,
    pub values: Vec<String>,
}

pub fn enrich_with_jdtls(
    project_root: &Path,
    options: &JdtlsOptions,
    model: &mut CodeModel,
) -> Result<(), String> {
    if !command_available(&options.command) {
        return Err(format!(
            "JDTLS executable not found.\ncommand: {}\nPATH: {}\nhint: install Eclipse JDT Language Server or pass --jdtls-command <path-to-jdtls>",
            options.command,
            env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
        ));
    }

    fs::create_dir_all(&options.workspace).map_err(|error| {
        format!(
            "failed to create JDTLS workspace {}: {error}",
            options.workspace.display()
        )
    })?;

    let max_in_flight = options.max_in_flight.max(1);
    let mut client = LspClient::start(&options.command, project_root, &options.workspace)?;
    client.initialize(project_root)?;
    client.open_java_files(project_root, model)?;
    client.require_document_symbols(project_root, model, max_in_flight)?;
    client.resolve_invocations(project_root, model, max_in_flight)?;
    client.resolve_references(project_root, model, max_in_flight.min(16))?;
    client.resolve_implementations(project_root, model, max_in_flight.min(16))?;
    client.shutdown();
    Ok(())
}

pub fn resolve_test_definitions(
    project_root: &Path,
    options: &JdtlsOptions,
    java_files: &[String],
    requests: &[JdtlsDefinitionRequest],
) -> Result<Vec<JdtlsDefinition>, String> {
    if !command_available(&options.command) {
        return Err(format!(
            "JDTLS executable not found.\ncommand: {}\nPATH: {}\nhint: install Eclipse JDT Language Server or pass --jdtls-command <path-to-jdtls>",
            options.command,
            env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
        ));
    }

    fs::create_dir_all(&options.workspace).map_err(|error| {
        format!(
            "failed to create JDTLS workspace {}: {error}",
            options.workspace.display()
        )
    })?;

    let max_in_flight = options.max_in_flight.max(1);
    let mut client = LspClient::start(&options.command, project_root, &options.workspace)?;
    client.initialize(project_root)?;
    client.open_java_file_paths(project_root, java_files)?;
    let definitions = client.resolve_definition_requests(project_root, requests, max_in_flight)?;
    client.shutdown();
    Ok(definitions)
}

pub fn resolve_compatibility_symbols(
    project_root: &Path,
    options: &JdtlsOptions,
    java_files: &[String],
    requests: &[JdtlsSymbolRequest],
) -> Result<Vec<JdtlsResolvedSymbol>, String> {
    if !command_available(&options.command) {
        return Err(format!(
            "JDTLS executable not found.\ncommand: {}\nPATH: {}\nhint: install Eclipse JDT Language Server or pass --jdtls-command <path-to-jdtls>",
            options.command,
            env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
        ));
    }

    fs::create_dir_all(&options.workspace).map_err(|error| {
        format!(
            "failed to create JDTLS workspace {}: {error}",
            options.workspace.display()
        )
    })?;

    let max_in_flight = options.max_in_flight.max(1);
    let mut client = LspClient::start(&options.command, project_root, &options.workspace)?;
    client.initialize(project_root)?;
    client.open_java_file_paths(project_root, java_files)?;
    let symbols =
        client.resolve_compatibility_symbol_requests(project_root, requests, max_in_flight)?;
    client.shutdown();
    Ok(symbols)
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists();
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(command).exists()))
        .unwrap_or(false)
}

struct LspClient {
    command: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: Option<ChildStderr>,
    next_id: i64,
}

struct LspResponse {
    id: i64,
    result: Result<Value, String>,
}

impl LspClient {
    fn start(command: &str, project_root: &Path, workspace: &Path) -> Result<Self, String> {
        let mut child = Command::new(command)
            .arg("-data")
            .arg(workspace)
            .current_dir(project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                format!(
                    "failed to start JDTLS.\ncommand: {command} -data {}\nproject: {}\nerror: {error}",
                    workspace.display(),
                    project_root.display()
                )
            })?;
        let stdin = child.stdin.take().ok_or("failed to capture JDTLS stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("failed to capture JDTLS stdout")?;
        let stderr = child.stderr.take();
        Ok(Self {
            command: command.to_string(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr,
            next_id: 1,
        })
    }

    fn initialize(&mut self, project_root: &Path) -> Result<(), String> {
        let root_uri = file_uri(project_root);
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "implementation": { "dynamicRegistration": false }
                },
                "workspace": { "workspaceFolders": true }
            },
            "workspaceFolders": [{
                "uri": root_uri,
                "name": project_root.file_name().and_then(|name| name.to_str()).unwrap_or("project")
            }]
        });
        self.request("initialize", params).map_err(|error| {
            self.verbose_error(
                "initialize",
                project_root,
                None,
                &format!("JDTLS initialize request failed: {error}"),
            )
        })?;
        self.notify("initialized", json!({})).map_err(|error| {
            self.verbose_error(
                "initialized",
                project_root,
                None,
                &format!("JDTLS initialized notification failed: {error}"),
            )
        })
    }

    fn open_java_files(&mut self, project_root: &Path, model: &CodeModel) -> Result<(), String> {
        for file in java_files(model) {
            self.open_java_file(project_root, &file, None)?;
        }
        Ok(())
    }

    fn open_java_file_paths(
        &mut self,
        project_root: &Path,
        files: &[String],
    ) -> Result<(), String> {
        for file in files {
            self.open_java_file(project_root, file, None)?;
        }
        Ok(())
    }

    fn open_java_file(
        &mut self,
        project_root: &Path,
        file: &str,
        module_id: Option<&str>,
    ) -> Result<(), String> {
        let module_label = module_id.unwrap_or("unknown");
        let path = project_root.join(file);
        let text = fs::read_to_string(&path).map_err(|error| {
            self.verbose_error(
                "didOpen",
                project_root,
                Some(file),
                &format!(
                    "failed to read Java source before JDTLS didOpen.\nmodule: {module_label}\nerror: {error}"
                ),
            )
        })?;
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": file_uri(&path),
                    "languageId": "java",
                    "version": 1,
                    "text": text
                }
            }),
        )
        .map_err(|error| {
            self.verbose_error(
                "didOpen",
                project_root,
                Some(file),
                &format!(
                    "JDTLS didOpen notification failed.\nmodule: {module_label}\nerror: {error}"
                ),
            )
        })?;
        Ok(())
    }

    fn resolve_definition_requests(
        &mut self,
        project_root: &Path,
        requests: &[JdtlsDefinitionRequest],
        max_in_flight: usize,
    ) -> Result<Vec<JdtlsDefinition>, String> {
        let total = requests.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending = BTreeMap::new();
        let mut definitions = Vec::new();
        log_phase_start("test definitions", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let request = requests[next].clone();
                next += 1;
                let path = project_root.join(&request.file);
                let id = self
                    .send_request(
                        "textDocument/definition",
                        json!({
                            "textDocument": { "uri": file_uri(&path) },
                            "position": {
                                "line": request.line.saturating_sub(1),
                                "character": request.column
                            }
                        }),
                    )
                    .map_err(|error| {
                        self.verbose_error(
                            "textDocument/definition",
                            project_root,
                            Some(&request.file),
                            &format!(
                                "JDTLS definition request failed for test call {} at {}:{}:{}.\nerror: {error}",
                                request.name, request.file, request.line, request.column
                            ),
                        )
                    })?;
                pending.insert(id, request);
            }

            let response = self.read_response()?;
            let Some(request) = pending.remove(&response.id) else {
                continue;
            };
            let result = response.result.map_err(|error| {
                self.verbose_error(
                    "textDocument/definition",
                    project_root,
                    Some(&request.file),
                    &format!(
                        "JDTLS definition request failed for test call {} at {}:{}:{}.\nerror: {error}",
                        request.name, request.file, request.line, request.column
                    ),
                )
            })?;
            for location in locations_from_value(&result) {
                if let Some(file) = relative_uri_path(project_root, &location.uri) {
                    definitions.push(JdtlsDefinition {
                        owner_id: request.owner_id.clone(),
                        name: request.name.clone(),
                        file,
                        line: location.line,
                    });
                }
            }
            complete += 1;
            log_phase_progress(
                "test definitions",
                complete,
                total,
                pending.len(),
                started_at,
            );
        }
        Ok(definitions)
    }

    fn resolve_compatibility_symbol_requests(
        &mut self,
        project_root: &Path,
        requests: &[JdtlsSymbolRequest],
        max_in_flight: usize,
    ) -> Result<Vec<JdtlsResolvedSymbol>, String> {
        let total = requests.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending = BTreeMap::new();
        let mut symbols = Vec::new();
        log_phase_start("compatibility definitions", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let request = requests[next].clone();
                next += 1;
                let path = project_root.join(&request.file);
                let id = self
                    .send_request(
                        "textDocument/definition",
                        json!({
                            "textDocument": { "uri": file_uri(&path) },
                            "position": {
                                "line": request.line.saturating_sub(1),
                                "character": request.column
                            }
                        }),
                    )
                    .map_err(|error| {
                        self.verbose_error(
                            "textDocument/definition",
                            project_root,
                            Some(&request.file),
                            &format!(
                                "JDTLS definition request failed for compatibility symbol {} at {}:{}:{}.\nerror: {error}",
                                request.name, request.file, request.line, request.column
                            ),
                        )
                    })?;
                pending.insert(id, request);
            }

            let response = self.read_response()?;
            let Some(request) = pending.remove(&response.id) else {
                continue;
            };
            let result = response.result.map_err(|error| {
                self.verbose_error(
                    "textDocument/definition",
                    project_root,
                    Some(&request.file),
                    &format!(
                        "JDTLS definition request failed for compatibility symbol {} at {}:{}:{}.\nerror: {error}",
                        request.name, request.file, request.line, request.column
                    ),
                )
            })?;
            let mut values = Vec::new();
            for location in locations_from_value(&result) {
                if relative_uri_path(project_root, &location.uri).is_none() {
                    values.extend(symbol_values_for_location(&request, &location));
                }
            }
            values.sort();
            values.dedup();
            if !values.is_empty() {
                symbols.push(JdtlsResolvedSymbol {
                    file: request.file,
                    line: request.line,
                    values,
                });
            }
            complete += 1;
            log_phase_progress(
                "compatibility definitions",
                complete,
                total,
                pending.len(),
                started_at,
            );
        }
        Ok(symbols)
    }

    fn require_document_symbols(
        &mut self,
        project_root: &Path,
        model: &mut CodeModel,
        max_in_flight: usize,
    ) -> Result<(), String> {
        let files = java_files(model);
        let total = files.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending: BTreeMap<i64, String> = BTreeMap::new();
        log_phase_start("document symbols", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let file = files[next].clone();
                next += 1;
                let path = project_root.join(&file);
                let id = self
                    .send_request(
                        "textDocument/documentSymbol",
                        json!({ "textDocument": { "uri": file_uri(&path) } }),
                    )
                    .map_err(|error| {
                        let module_id = module_id_for_file(&file, &model.modules);
                        self.verbose_error(
                            "textDocument/documentSymbol",
                            project_root,
                            Some(&file),
                            &format!(
                                "JDTLS document symbols request failed.\nmodule: {module_id}\nerror: {error}"
                            ),
                        )
                    })?;
                pending.insert(id, file);
            }

            let response = self.read_response()?;
            let Some(file) = pending.remove(&response.id) else {
                continue;
            };
            let result = response.result.map_err(|error| {
                let module_id = module_id_for_file(&file, &model.modules);
                self.verbose_error(
                    "textDocument/documentSymbol",
                    project_root,
                    Some(&file),
                    &format!(
                        "JDTLS document symbols request failed.\nmodule: {module_id}\nerror: {error}"
                    ),
                )
            })?;
            if result.is_null() {
                model.diagnostics.push(Diagnostic::warning(
                    "jdtls",
                    format!("JDTLS returned no document symbols for {file}"),
                    Some(file),
                ));
            }
            complete += 1;
            log_phase_progress(
                "document symbols",
                complete,
                total,
                pending.len(),
                started_at,
            );
        }
        Ok(())
    }

    fn resolve_invocations(
        &mut self,
        project_root: &Path,
        model: &mut CodeModel,
        max_in_flight: usize,
    ) -> Result<(), String> {
        let invocations = model.invocations.clone();
        let total = invocations.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending = BTreeMap::new();
        log_phase_start("definitions", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let invocation = invocations[next].clone();
                next += 1;
                let path = project_root.join(&invocation.file);
                let id = self
                    .send_request(
                        "textDocument/definition",
                        json!({
                            "textDocument": { "uri": file_uri(&path) },
                            "position": {
                                "line": invocation.line.saturating_sub(1),
                                "character": invocation.column
                            }
                        }),
                    )
                    .map_err(|error| {
                        definition_error(self, project_root, model, &invocation, &error)
                    })?;
                pending.insert(id, invocation);
            }

            let response = self.read_response()?;
            let Some(invocation) = pending.remove(&response.id) else {
                continue;
            };
            let result = response.result.map_err(|error| {
                definition_error(self, project_root, model, &invocation, &error)
            })?;
            for location in locations_from_value(&result) {
                if let Some(target_id) = method_id_for_location(project_root, model, &location) {
                    model.relationships.push(RelationshipInfo {
                        source_id: invocation.caller_method_id.clone(),
                        target_id,
                        kind: "CALLS".to_string(),
                        confidence: 0.95,
                        source: "jdtls".to_string(),
                    });
                }
            }
            complete += 1;
            log_phase_progress("definitions", complete, total, pending.len(), started_at);
        }
        Ok(())
    }

    fn resolve_references(
        &mut self,
        project_root: &Path,
        model: &mut CodeModel,
        max_in_flight: usize,
    ) -> Result<(), String> {
        let methods = model.methods.clone();
        let total = methods.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending = BTreeMap::new();
        log_phase_start("references", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let method = methods[next].clone();
                next += 1;
                let path = project_root.join(&method.file);
                let id = self
                    .send_request(
                        "textDocument/references",
                        json!({
                            "textDocument": { "uri": file_uri(&path) },
                            "position": {
                                "line": method.name_line.saturating_sub(1),
                                "character": method.name_column
                            },
                            "context": { "includeDeclaration": false }
                        }),
                    )
                    .map_err(|error| references_error(self, project_root, &method, &error))?;
                pending.insert(id, method);
            }

            let response = self.read_response()?;
            let Some(method) = pending.remove(&response.id) else {
                continue;
            };
            let result = response
                .result
                .map_err(|error| references_error(self, project_root, &method, &error))?;
            for location in locations_from_value(&result) {
                let Some(source_id) = method_id_for_location(project_root, model, &location) else {
                    continue;
                };
                if source_id != method.id {
                    model.relationships.push(RelationshipInfo {
                        source_id,
                        target_id: method.id.clone(),
                        kind: "REFERENCES".to_string(),
                        confidence: 0.90,
                        source: "jdtls".to_string(),
                    });
                }
            }
            complete += 1;
            log_phase_progress("references", complete, total, pending.len(), started_at);
        }
        Ok(())
    }

    fn resolve_implementations(
        &mut self,
        project_root: &Path,
        model: &mut CodeModel,
        max_in_flight: usize,
    ) -> Result<(), String> {
        let methods = model.methods.clone();
        let total = methods.len();
        let started_at = Instant::now();
        let mut next = 0;
        let mut complete = 0;
        let mut pending = BTreeMap::new();
        log_phase_start("implementations", total, max_in_flight);

        while next < total || !pending.is_empty() {
            while pending.len() < max_in_flight && next < total {
                let method = methods[next].clone();
                next += 1;
                let path = project_root.join(&method.file);
                let id = self
                    .send_request(
                        "textDocument/implementation",
                        json!({
                            "textDocument": { "uri": file_uri(&path) },
                            "position": {
                                "line": method.name_line.saturating_sub(1),
                                "character": method.name_column
                            }
                        }),
                    )
                    .map_err(|error| implementations_error(self, project_root, &method, &error))?;
                pending.insert(id, method);
            }

            let response = self.read_response()?;
            let Some(method) = pending.remove(&response.id) else {
                continue;
            };
            let result = response
                .result
                .map_err(|error| implementations_error(self, project_root, &method, &error))?;
            for location in locations_from_value(&result) {
                let Some(target_id) = method_id_for_location(project_root, model, &location) else {
                    continue;
                };
                if target_id != method.id {
                    model.relationships.push(RelationshipInfo {
                        source_id: method.id.clone(),
                        target_id,
                        kind: "IMPLEMENTED_BY".to_string(),
                        confidence: 0.90,
                        source: "jdtls".to_string(),
                    });
                }
            }
            complete += 1;
            log_phase_progress(
                "implementations",
                complete,
                total,
                pending.len(),
                started_at,
            );
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.send_request(method, params)?;
        loop {
            let response = self.read_response()?;
            if response.id != id {
                continue;
            }
            return response.result;
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> Result<i64, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        Ok(id)
    }

    fn read_response(&mut self) -> Result<LspResponse, String> {
        loop {
            let message = self.read_message()?;
            if message.get("method").is_some() {
                continue;
            }
            let Some(id) = message.get("id").and_then(Value::as_i64) else {
                continue;
            };
            if let Some(error) = message.get("error") {
                return Ok(LspResponse {
                    id,
                    result: Err(error.to_string()),
                });
            }
            return Ok(LspResponse {
                id,
                result: Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            });
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    fn write_message(&mut self, value: &Value) -> Result<(), String> {
        let body = serde_json::to_string(value)
            .map_err(|error| format!("failed to serialize LSP message: {error}"))?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
            .map_err(|error| format!("failed to write LSP message to JDTLS stdin: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush JDTLS stdin: {error}"))
    }

    fn read_message(&mut self) -> Result<Value, String> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .map_err(|error| format!("failed to read JDTLS stdout header: {error}"))?;
            if read == 0 {
                return Err("JDTLS stdout closed before a complete LSP response".to_string());
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                    format!("invalid JDTLS Content-Length header {trimmed:?}: {error}")
                })?);
            }
        }
        let length = content_length.ok_or("JDTLS response missing Content-Length header")?;
        let mut body = vec![0; length];
        self.stdout
            .read_exact(&mut body)
            .map_err(|error| format!("failed to read JDTLS response body: {error}"))?;
        serde_json::from_slice(&body)
            .map_err(|error| format!("failed to parse JDTLS JSON response: {error}"))
    }

    fn verbose_error(
        &mut self,
        phase: &str,
        project_root: &Path,
        file: Option<&str>,
        message: &str,
    ) -> String {
        let mut details = vec![
            message.to_string(),
            format!("phase: {phase}"),
            format!("command: {} -data <workspace>", self.command),
            format!("project: {}", project_root.display()),
        ];
        if let Some(file) = file {
            details.push(format!("file: {file}"));
        }
        if let Ok(Some(status)) = self.child.try_wait() {
            details.push(format!("jdtls_exit_code: {}", status.code().unwrap_or(1)));
            if let Some(stderr) = self.read_stderr_excerpt() {
                details.push(format!("jdtls_stderr: {stderr}"));
            }
        }
        details.join("\n")
    }

    fn read_stderr_excerpt(&mut self) -> Option<String> {
        let mut stderr = self.stderr.take()?;
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(800).collect())
    }

    fn shutdown(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn java_files(model: &CodeModel) -> Vec<String> {
    model
        .methods
        .iter()
        .map(|method| method.file.clone())
        .chain(model.classes.iter().map(|class| class.file.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn definition_error(
    client: &mut LspClient,
    project_root: &Path,
    model: &CodeModel,
    invocation: &InvocationInfo,
    error: &str,
) -> String {
    let module_id = module_id_for_file(&invocation.file, &model.modules);
    client.verbose_error(
        "textDocument/definition",
        project_root,
        Some(&invocation.file),
        &format!(
            "JDTLS definition request failed for call {} at {}:{}:{}.\nmodule: {module_id}\nerror: {error}",
            invocation.name, invocation.file, invocation.line, invocation.column
        ),
    )
}

fn references_error(
    client: &mut LspClient,
    project_root: &Path,
    method: &MethodInfo,
    error: &str,
) -> String {
    let module_id = &method.module_id;
    client.verbose_error(
        "textDocument/references",
        project_root,
        Some(&method.file),
        &format!(
            "JDTLS references request failed for method {} at {}:{}:{}.\nmodule: {module_id}\nerror: {error}",
            method.id, method.file, method.name_line, method.name_column
        ),
    )
}

fn implementations_error(
    client: &mut LspClient,
    project_root: &Path,
    method: &MethodInfo,
    error: &str,
) -> String {
    let module_id = &method.module_id;
    client.verbose_error(
        "textDocument/implementation",
        project_root,
        Some(&method.file),
        &format!(
            "JDTLS implementation request failed for method {} at {}:{}:{}.\nmodule: {module_id}\nerror: {error}",
            method.id, method.file, method.name_line, method.name_column
        ),
    )
}

fn log_phase_start(phase: &str, total: usize, max_in_flight: usize) {
    eprintln!("jdtls {phase}: total={total} max_in_flight={max_in_flight}");
}

fn log_phase_progress(
    phase: &str,
    complete: usize,
    total: usize,
    in_flight: usize,
    started_at: Instant,
) {
    if complete == total || complete % 250 == 0 {
        eprintln!(
            "jdtls {phase}: {complete}/{total} complete in_flight={in_flight} elapsed_ms={}",
            started_at.elapsed().as_millis()
        );
    }
}

#[derive(Debug)]
struct LspLocation {
    uri: String,
    line: usize,
}

fn locations_from_value(value: &Value) -> Vec<LspLocation> {
    match value {
        Value::Array(values) => values.iter().filter_map(location_from_value).collect(),
        Value::Object(_) => location_from_value(value).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn location_from_value(value: &Value) -> Option<LspLocation> {
    let uri = value.get("uri")?.as_str()?.to_string();
    let line = value.get("range")?.get("start")?.get("line")?.as_u64()? as usize + 1;
    Some(LspLocation { uri, line })
}

fn symbol_values_for_location(request: &JdtlsSymbolRequest, location: &LspLocation) -> Vec<String> {
    let mut values = Vec::new();
    let Some(class_name) = class_name_from_uri(&location.uri) else {
        return values;
    };
    values.push(class_name.clone());
    if let Some(method) = method_name_from_values(&request.values) {
        values.push(format!("{class_name}.{method}"));
        if class_name == "java.lang.Class" && method.starts_with("forName(") {
            values.extend(
                request
                    .values
                    .iter()
                    .filter(|value| value.starts_with("Class.forName(\""))
                    .cloned(),
            );
        }
        if class_name == "java.lang.reflect.AccessibleObject"
            && request
                .values
                .iter()
                .any(|value| value == "setAccessible(true)")
        {
            values.push("setAccessible(true)".to_string());
        }
    }
    values
}

fn method_name_from_values(values: &[String]) -> Option<String> {
    for value in values {
        let Some((before_args, args)) = value.split_once('(') else {
            continue;
        };
        let Some(method) = before_args.rsplit('.').next().map(str::trim) else {
            continue;
        };
        if !method.is_empty() {
            return Some(format!("{method}({args}"));
        }
    }
    None
}

fn class_name_from_uri(uri: &str) -> Option<String> {
    let marker = uri.find(".class").or_else(|| uri.find(".java"))?;
    let before = &uri[..marker];
    for package in ["java/", "javax/", "jdk/", "sun/", "com/sun/", "org/omg/"] {
        if let Some(index) = before.find(package) {
            return Some(before[index..].replace('/', "."));
        }
    }
    None
}

fn method_id_for_location(
    project_root: &Path,
    model: &CodeModel,
    location: &LspLocation,
) -> Option<String> {
    let relative = relative_uri_path(project_root, &location.uri)?;
    model
        .methods
        .iter()
        .find(|method| {
            method.file == relative
                && method.start_line <= location.line
                && method.end_line >= location.line
        })
        .map(|method| method.id.clone())
}

fn file_uri(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!(
        "file://{}",
        percent_encode_path(&absolute.to_string_lossy())
    )
}

fn percent_encode_path(path: &str) -> String {
    path.chars()
        .flat_map(|ch| match ch {
            ' ' => "%20".chars().collect::<Vec<_>>(),
            '#' => "%23".chars().collect(),
            '%' => "%25".chars().collect(),
            '?' => "%3F".chars().collect(),
            _ => vec![ch],
        })
        .collect()
}

fn relative_uri_path(root: &Path, uri: &str) -> Option<String> {
    let path = uri.strip_prefix("file://")?;
    let decoded = percent_decode(path);
    let path = PathBuf::from(decoded);
    Some(
        path.strip_prefix(root)
            .ok()?
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn percent_decode(value: &str) -> String {
    value
        .replace("%20", " ")
        .replace("%23", "#")
        .replace("%3F", "?")
        .replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_symbols_extract_jdk_class_and_method() {
        let request = JdtlsSymbolRequest {
            file: "src/main/java/demo/Demo.java".to_string(),
            name: "currentThread()".to_string(),
            line: 1,
            column: 42,
            values: vec!["currentThread()".to_string()],
        };
        let location = LspLocation {
            uri: "jdt://contents/java.base/java/lang/Thread.class".to_string(),
            line: 1,
        };

        let values = symbol_values_for_location(&request, &location);

        assert!(values.contains(&"java.lang.Thread".to_string()));
        assert!(values.contains(&"java.lang.Thread.currentThread()".to_string()));
    }

    #[test]
    fn compatibility_symbols_keep_reflective_literal_after_class_for_name_resolves() {
        let request = JdtlsSymbolRequest {
            file: "src/main/java/demo/Demo.java".to_string(),
            name: "Class.forName(\"sun.misc.Unsafe\")".to_string(),
            line: 1,
            column: 42,
            values: vec![
                "Class.forName(\"sun.misc.Unsafe\")".to_string(),
                "forName(\"sun.misc.Unsafe\")".to_string(),
            ],
        };
        let location = LspLocation {
            uri: "jdt://contents/java.base/java/lang/Class.class".to_string(),
            line: 1,
        };

        let values = symbol_values_for_location(&request, &location);

        assert!(values.contains(&"Class.forName(\"sun.misc.Unsafe\")".to_string()));
    }

    #[test]
    fn compatibility_symbols_gate_set_accessible_to_reflection_api() {
        let request = JdtlsSymbolRequest {
            file: "src/main/java/demo/Demo.java".to_string(),
            name: "setAccessible(true)".to_string(),
            line: 1,
            column: 42,
            values: vec!["setAccessible(true)".to_string()],
        };
        let location = LspLocation {
            uri: "jdt://contents/java.base/java/lang/reflect/AccessibleObject.class".to_string(),
            line: 1,
        };

        let values = symbol_values_for_location(&request, &location);

        assert!(values.contains(&"setAccessible(true)".to_string()));
    }
}
