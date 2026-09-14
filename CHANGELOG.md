# Change Log

All notable changes to the "Qlue-ls" project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- predicate completions inside a blank node property list (`?s :p [ | ]`) now
  take the surrounding query into account: the completion query is constrained
  by the triples connected to the enclosing subject, including the other
  properties of the enclosing triple. A context insensitive query is dispatched
  alongside it as a fallback.

## [3.9.0] - 2026-09-09

### Added

- folding range for frontmamtter comment
  A Frontmatter comment is the first comment in the file. It MUST start with `#+`
  and documents metadatat about the query.
  i.e.
  #+ title: My query
  #+ description: queries stuff

### Changed

- the templating engine for completion and hover queries was updated to Tera
  v2. Two changes affect existing query templates:

  - numeric field access is gone: `{{ prefix.0 }}` has to be written as
    `{{ prefix[0] }}`. This only matters if you iterate `prefixes` yourself
    instead of using `{% include "prefix_declarations" %}`.
  - macros (`{% macro %}` / `{% import %}`) were removed in favor of
    components, see the [Tera migration
    guide](https://github.com/Keats/tera/blob/master/MIGRATION.md).

  Everything else in the documented templates keeps working, including
  `{% if search_term_uncompressed %}` on an undefined variable.

- a template that fails to load or render now reports *why*: the error message
  contains Tera's source-annotated report pointing at the offending part of the
  template.
- quickfixes insert predicate declarations after the first comments

## [3.8.0] - 2026-09-05

### Added

- diagnostics are now reported for update operations (`INSERT`, `DELETE`,
  `LOAD`, `CLEAR`, ...). Previously a diagnostic request on an update failed
  with "diagnostics are currently only supported for query operations".

### Fixed

- keyword completions at the start of a query are offered again when the query
  begins with a comment.

## [3.7.0] - 2026-09-04

### Added

- completion queries can bind an optional `?qls_description` to a prose
  description of the entity (e.g. "large city in Baden-Württemberg, Germany").
  It is sent as the completion item's `detail` and in the `qlueLs` data payload
  as `description`.

### Changed

- a completion item's `qlueLs` data payload carries a single `alias` string
  instead of an `aliases` list, and result rows are no longer grouped by
  entity: one row is one completion item. A completion query is expected to
  return an entity once, binding at most the one `?qls_alias` that matched the
  search term — collecting every alias only made the query slow.

  To migrate, read `data.qlueLs.alias` as an optional string where you read
  `data.qlueLs.aliases`, and drop any `GROUP_CONCAT` over aliases from your
  completion queries — `SAMPLE(?alias)` on a query that already filters aliases
  by the search term is enough.
- the `qlueLs/completionQuery` notification now carries the `requestId` of the
  completion request it belongs to, so clients can group the queries of one
  request. Note that a request can send several notifications, or none.

## [3.6.0] - 2026-09-02

### Changed

- entity and literal completion items now carry their facts in
  `CompletionItem.data` under a `qlueLs` key instead of encoding them into
  `labelDetails.detail` and `documentation`. An entity carries its label, its
  aliases (as a list) and its usage score; a literal carries its lexical form,
  its language and its datatype. `detail` and `documentation` are now plain
  human readable fields again, and `sortText` stays the authoritative ranking.
- an entity whose aliases arrive across several bindings is now reported as a
  single completion item, and aliases packed into one binding with
  `GROUP_CONCAT(?alias; SEPARATOR="\t")` are split apart.

### Fixed

- static subject completions (such as `Sub select`) now carry a text edit over
  the term being completed, so accepting them no longer leaves the already
  typed prefix in front of the inserted snippet.

## [3.5.0] - 2026-08-31

### Added

- new `qlueLs/completionQuery` notification (web assembly only), sent after
  every completion query. It carries the rendered query, the endpoint URL, the
  duration and either the number of returned bindings or the reason the query
  failed. Clients can use it to inspect completion queries while a completion
  template is being edited; previously the rendered query was only written to
  the log.

## [3.4.4] - 2026-08-31

### Fixed

- online completion items are now ordered by their `?qls_count` relevance score
  instead of the order the SPARQL endpoint happened to return them in. Items
  without a score are listed last.
- a partially typed multi word keyword no longer loses its completions. The
  first word lexes as that keyword's own token, so as soon as a space followed
  it (`GROUP `, `GROUP B`) the location came out as `Unknown` and no
  completions were offered at all.
- solution modifier completions (`GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`,
  `OFFSET`) now carry a text edit over the range they replace instead of a bare
  insert text, so clients know where the term being completed starts.

## [3.4.3] - 2026-08-29

### Fixed

- semantic tokens are now reported with UTF-16 character offsets, as required by
  the LSP specification. Highlighting no longer drifts on lines containing
  non-ASCII characters.

## [3.4.2] - 2026-08-21

### Fixed

- fix serializer for QLever update responses
- completions inside inverse property paths (`^`) now suggest the correct
  predicates again

## [3.4.1] - 2027-07-30

### Fixed

- `qlueLs/executeOperation` no longer fails on `ASK` queries. The boolean
  SPARQL result is now parsed and returned; pagination and lazy loading are
  disabled for it, since the result is a single value.

### Changed

- `qlueLs/pingBackend` now checks availability in an engine-aware way: QLever
  backends are pinged via their `/ping` endpoint, all other engines receive a
  minimal SPARQL query. An explicitly configured `healthCheckUrl` is still
  requested as is. The check times out after 5 seconds and accepts any 2xx
  response.

## [3.3.2] - 2027-07-26

### Fixed

- an invalid regex in `replacements.objectVariable` no longer panics the server
  on the next completion request. Such a pattern is rejected when it arrives via
  `qlueLs/changeSettings`, and skipped with a warning when it comes from a
  configuration file.

### Changed

- `qlueLs/changeSettings` now merges the received settings into the current ones
  instead of replacing them. Only the keys that change have to be sent, omitted
  keys keep their value. Objects are merged recursively, arrays are replaced, and
  an explicit `null` unsets an optional section.

- variable completions in object position now keep the word boundaries of the
  predicate name, instead of gluing the words together. `wdt:hasBirthDate` and
  `has birth date` both suggest `?birth_date` where they previously suggested
  `?birthdate`. The `replacements.objectVariable` defaults changed accordingly,
  configured replacements are unaffected.

## [3.3.1] - 2027-07-21

### Added

- folding ranges (`textDocument/foldingRange`) for every block delimited by
  curly brackets, not just the top-level query body.

### Fixed

- the "contract triples" code action no longer panics when a triple in the
  group has no properties list path.

## [3.3.0] - 2027-07-15

### Added

- rename support (`textDocument/rename`): renaming a variable updates all
  occurrences that denote the same variable, respecting scope boundaries —
  sub-selects connect only through projected variables, and `UNION` branches
  are treated as separate scopes unless bridged by an outer occurrence. The
  new name is validated against the SPARQL `VARNAME` grammar, and a leading
  `?` or `$` is tolerated.
- find references support (`textDocument/references`): lists all occurrences
  of the variable under the cursor, using the same scope rules as rename.
- document highlight support (`textDocument/documentHighlight`): placing the
  cursor on a variable highlights all its occurrences in the query.

### Fixed

- on-type formatting no longer panics when a newline is typed inside an `ANON`
  (`[ ]`) or `NIL` (`( )`) token, where the lexer folds the whitespace into the
  token itself. Newlines inside any other token (e.g. long string literals) are
  now left untouched instead of being re-indented.

## [3.2.1] - 2026-07-09

### Added

- variable completions in `GROUP BY` clauses: after `GROUP BY` (and while
  typing further `?`-prefixed conditions) the variables visible in the query
  are suggested, excluding those already grouped. Works in nested sub-selects,
  where only the variables of the enclosing sub-select are offered. Solution
  modifier keywords are no longer suggested in this position.
- new diagnostic `groupby-star-selection` (error): flags `SELECT *` in queries
  with a `GROUP BY` clause, where only grouped variables or aggregates may be
  selected. Sub-selects are checked as well.

## [3.2.0] - 2026-07-06

### Changed

- **breaking**: `qlueLs/jump` now formats the document server-side and returns
  `{ edits, position }`: the edits (format + placeholder insertions) against
  the request-time document and the final cursor position after applying them.
  This replaces the previous `{ position, insertBefore, insertAfter }` result
  and fixes the cursor drifting when format edits removed the line it was on.
  `JumpParams` accepts optional `FormattingOptions` as `options`.

## [3.1.1] - 2026-07-05

### Changed

- aggregate completions in the `SELECT` clause now work for partially typed
  aggregates: typing e.g. `SELECT (S` suggests the matching
  `(SUM(?var) AS ?alias)` and `(SAMPLE(?var) AS ?alias)` snippets, replacing
  the already typed text. A `GROUP_CONCAT` snippet is now offered as well.
- completions can now look past the cursor: variable completion in the
  `SELECT` clause suggests the variables of the `WHERE` clause, even though
  it comes after the cursor.
- syntax diagnostics are more precise: the parser now recovers from errors at
  structural keywords (e.g. `WHERE`, braces), so a single mistake no longer
  cascades into errors for the rest of the query. Diagnostics point at the
  offending token instead of the end of the previous one, and errors at the
  end of the input (e.g. a missing closing brace) are now reported instead of
  silently dropped.

## [3.1.0] - 2026-07-02

### Added

- new `duplicate-prefix-declaration` diagnostic that warns when a prefix is
  declared more than once in the prologue, with a quickfix to remove the
  redundant declaration.
- operation error responses now include a new `Http` error type carrying the
  status code, status text, and raw response body when the endpoint returns a
  non-2xx response without a structured error message (e.g. an html 404 page).
  Previously such failures were reported as an unknown error.

### Changed

- **BREAKING** the `Connection` operation error's `statusText` field is renamed
  to `message`, since it holds the underlying network error, not an http
  status. Clients reading `statusText` from connection errors must be updated.
- connection errors now report the actual browser/network error message (e.g.
  `NetworkError when attempting to fetch resource.`) instead of a
  debug-formatted JsValue.
- on native targets, failed requests now report structured errors (including
  QLever error messages) with status code and body, instead of a generic
  "failed" message.

- **BREAKING** template variables are now prefixed with `qls_` instead of
  `qlue_ls_` for conciseness. Existing query templates using the `qlue_ls_`
  prefix must be updated to the new `qls_` prefix.

### Fixed

- the language server no longer crashes when a completion query result is
  missing the `qls_entity` binding. It now returns an error response instead.
- object completions no longer append a triple terminator or a stray trailing
  space when the triple is already terminated by a `.` or continued with a `;`.

## [2.8.2] - 2026-06-11

### Fixed

- `textDocument/didChange` now accepts content changes without a `range`,
  which per the LSP specification replace the entire document. Previously
  such notifications failed to deserialize, so clients using full document
  synchronization could not update documents.

## [2.8.1] - 2026-06-11

### Fixed

- the `ungrouped-select-var` diagnostic now recognizes a bracketted variable
  in the GROUP BY clause (e.g. `GROUP BY (?x)`) as grouping by that variable.
  Complex expressions like `GROUP BY (1 + ?x)` still do not make `?x`
  projectable.

## [2.8.0] - 2026-05-29

### Added

- SPARQL 1.2 support. The language server now understands the new syntax and
  vocabulary introduced by SPARQL 1.2 / RDF 1.2:
  - **Parser** — recognizes the new grammar constructs:
    - version declarations (`VERSION "1.2"`)
    - reified triples (`<< :s :p :o >>`) with optional reifiers (`~`)
    - triple terms (`<<( :s :p :o )>>`), in data, patterns and expressions
    - annotation syntax with annotation blocks (`{| :p :o |}`)
    - directional language tags (e.g. `"text"@en--ltr`)
  - **Syntax highlighting** — semantic tokens are emitted for the new
    keywords and constructs, so 1.2 queries are highlighted correctly.
  - **Completions** — the built-in function list now offers the new functions:
    `TRIPLE`, `SUBJECT`, `PREDICATE`, `OBJECT` and `isTRIPLE` for triple terms,
    and `LANGDIR`, `STRLANGDIR`, `hasLANG` and `hasLANGDIR` for directional
    language strings.

## [2.7.1] - 2026-05-23

### Fixed

- the `ungrouped-select-var` diagnostic no longer flags a variable used in a
  SELECT-clause assignment when that variable was derived by an earlier
  assignment in the same SELECT clause. Forward references to variables derived
  in later assignments are still reported.

## [2.7.0] - 2026-05-16

### Added

- `qlueLs/executeOperation` now accepts an inline `query` string as an
  alternative to `textDocument`, letting clients submit a SPARQL operation
  without first synchronizing it as a document. The existing `textDocument`
  form continues to work unchanged.
- lazy query evaluation now reports the total result count back to the
  client. QLever responses already include a trailing `meta` block; for
  every other engine a synthetic `Meta` partial-result with
  `result-size-total` is emitted once the stream completes.

## [2.6.5] - 2026-04-28

- parser crash on empty variables

## [2.6.4] - 2026-04-28

### Fixed

- update response deserialization for `DELETE WHERE` operations, where
  `deleteTriples` carries the detailed timing breakdown and `insertTriples`
  is a plain `0` (QLever specific)

## [2.6.3] - 2026-04-27

### Fixed

- Adapt to new JSON format for updates (QLever specific)

## [2.6.2] - 2026-04-27

### Fixed

- completion, hover and other position-based features now work correctly inside incomplete queries:
  the parser preserves the structural identity of the rule being parsed when input ends unexpectedly,
  instead of collapsing it into a generic error node
- completion now triggers correctly when the cursor is at the very end of the document

## [2.6.1] - 2026-04-23

### Fixed

- tokenization of `NIL`

### Added

- completions for all SPARQL 1.1 built-in functions with documentation and signatures

## [2.6.0] - 2026-04-10

### Added

- semantic tokens support for syntax highlighting

## [2.5.4] - 2026-04-09

### Changed

- fall back to default backend for completions

### Fixed

- completion search term computation
- collect prefix usages for all query types

## [2.5.3] - 2026-03-01

### Changed

- improved parser error handling

### Fixed

- formatting no longer inserts trailing linebreaks before `Error` nodes in incomplete/invalid syntax

## [2.5.2] - 2026-02-28

### Fixed

- formatting with `separate_prologue: true` and `keep_empty_lines: false`: the linebreak
  after the prologue was not inserted when the source contained extra blank lines

## [2.5.1] - 2026-02-28

### Added

- object variable completions based on subject AND predicate labels.
  For example `<Albert> <has_lastname> |` -> `?alber_lastname`

### Fixed

- idempotence of formatting with `separate_prologue` + `keep_empty_lines` options enabled

## [2.5.0] - 2026-02-19

### Added

- New `format.keep_empty_lines` setting: when enabled, preserves intentional blank lines from the original source. Consecutive blank lines are collapsed into a single empty line. Disabled by default.

### Fixed

- removed newline generation in construct formattig

## [2.4.0] - 2026-02-18

### Added

- New `auto_line_break` setting: when enabled, typing `;` or `.` after a valid triple automatically inserts a newline with correct indentation. Disabled by default.

## [2.3.0] - 2026-02-18

### Added

- `textDocument/onTypeFormatting`: pressing Enter after a `;` in a triple pattern automatically indents the new line to align with the first predicate (when `align_predicates = true`), or one tab unit beyond the brace-depth indent (when `align_predicates = false`).

## [2.2.1] - 2026-02-15

### Fixed

- Restored `prefix_declarations` Tera template that was accidentally removed in 2.2.0.

## [2.2.0] - 2026-02-15

### Added

- `qlueLs/parseTree` request to retrieve the full parse tree for a document, with optional `skipTrivia` parameter to exclude whitespace and comment tokens.
- Completions inside `VALUES` clauses with multi-variable positional tracking and context-sensitive/insensitive query variants.

## [2.0.1] - 2026-02-09

### Fixed

- tokenization of bare numeric literals in contruct responses.

## [2.0.0] - 2026-02-08

### Changed

- **BREAKING**: Backend configuration no longer uses a nested `service` object. The fields `name`, `url`, `healthCheckUrl`, and `engine` are now top-level properties of a backend configuration. This affects both the configuration file (`qlue-ls.toml`/`qlue-ls.yml`) and the `qlueLs/addBackend` notification params.
- **BREAKING**: `qlueLs/getBackend` now returns the full backend configuration (including `requestMethod`, `prefixMap`, `default`, `queries`, `additionalData`) instead of only the service fields. It also returns an `error` field when no default backend is configured.
- **BREAKING**: `qlueLs/listBackends` response items now contain `name`, `url`, and `default` only. The `healthCheckUrl` and `engine` fields are no longer included in list results.

### Added

- New optional `additionalData` field on backend configuration.

## [1.1.19] - 2026-02-01

### Added

- contract triples on format (configurable)
- new code-action: contract all triples with same subject at once

## [1.1.18] - 2026-01-25

### Added

- report syntax errors

### Fixed

- variable completions use text_edits instead of insert_text

### Changed

- reduced footprint of WASM target

## [1.1.17] - 2026-01-24

### Added

- new configuration option `format.line_length` to control when SELECT clauses break across multiple lines

## [1.1.16] - 2026-01-23

### Added

- new configuration option `completion.same_subject_semicolon` to control whether subject completions matching the previous subject transform the trailing dot to a semicolon
- experimental feature: compact formatting

## [1.1.14] - 2026-01-22

### Changed

- If no limit is provided for "qlueLs/executeOperation" request, the full result is returned

## [1.1.13] - 2026-01-22

### Added

- new "qlueLs/listBackends" method to list loaded SPARQL services

## [1.1.12] - 2026-01-21

### Fixed

- use provided window in query execution requests

### Changed

- renamed query template variable "qlue_ls_detail" to "qlue_ls_alias"

## [1.1.11] - 2026-01-20

### Changed

- remove "Rank" from completion item documentation
- trigger completion after object completion if object_completion_suffix is enabled

## [1.1.10] - 2026-01-19

### Added

- option to add suffix " .\n" to object completion queries
- option to have a minimum of chars before subject online completions are triggered

## [1.1.9] - 2026-01-18

### Added

- code action: add rdfs:label for variable

### Changed

- use interior mutability for parse tree cache

## [1.1.7] - 2026-01-17

### Fixed

- lexer for blank node label

## [1.1.5] - 2026-01-17

### Added

- extra information in the documentation of completion items

## [1.1.4] - 2026-01-16

### Changed

- keyword completions (FILTER, BIND, OPTIONAL, etc.) are now filtered by search term prefix

## [1.1.3] - 2026-01-16

### Added

- documentation field to CompletionItem for richer completion hints

### Changed

- refactored completion item label and detail rendering

### Fixed

- aggregate completion variable
- completion trigger token detection at end of query with trailing empty nodes

## [1.1.2] - 2026-01-13

### Fixed

- spaces in variable name completions

## [1.1.1] - 2026-01-13

### Changed

- capitalize snippet label

### Fixed

- "remove prefix declaration" quickfix

## [1.1.0] - 2025-12-27

### Added

- access token to qlueLs/executeOperation request

## [1.0.0] - 2025-12-27

### BREAKING

The request for "qlueLs/executeQuery" has been renamed to "qlueLs/executeOperation".  
The result schema of this request also changed.

### added

- construct support
- update support

### fixed

- adjust to breaking changes in the parser

## [0.24.1] - 2025-12-26

### fixed

- handle cancel notification mid result stream

## [0.24.0] - 2025-12-26

### added

- "qlueLs/cancelQuery" notification to cancel a running query

## [0.23.1] - 2025-12-17

### fixed

- lazy sparql result reader

## [0.23.0] - 2025-12-14

### Added

- more sparql engines
- lazy sparql result reader

### Fixed

- read qlever exception

## [0.21.0] - 2025-12-09

### Changed

- rename backend to service in qlueLs/addBackend

## [0.20.3] - 2025-11-26

### Added

- CRLF support

### Changed

- updated rust edition to 2024

## [0.20.2] - 2025-11-25

### Fixed

- apply changes from textDocument/didChange correctly, again

## [0.20.1] - 2025-11-24

### Fixed

- predicate and object completion for non-wasm targets. (thanks to @DeaconDesperado)
- completion replacement for neovim (thanks to @DeaconDesperado)
- apply changes from textDocument/didChange correctly

### Added

- set Query-Id for executeQuery requests
- qlueLs/getBackend request to get current default backend

## [0.19.2] - 2025-11-06

### Added

- post message if wasm-target crashes

## [0.19.1] - 2025-11-03

### Added

- forward connection error for executeQuery requests
- forward SPARQL endpoint error for executeQuery requests

### Fixed

- formatting blank-node-property-list

## [0.18.0] - 2025-10-29

### Fixed

- completion queries in demo

### Added

- capability to execute queries


## [0.17.1] - 2025-10-21

### Fixed

- object completions now use correct query templates

## [0.17.0] - 2025-10-18

### Changed

- renamed completion query templates (BREAKING)

### Added

- foldingRange for Prologue

### Fixed

- formatting emojis
- indentation after contract same subject triples

## [0.15.1] - 2025-09-30

### Changed

- core textedit apply algorithm

### Added

- more hover documentation for keywords
- aggregate completions for implicit GROUP BY

## [0.14.2] - 2025-09-23

### Added

- aggregate completions

### Fixed

- prevent trailing newline for monaco based editors

## [0.14.1]

### Fixed

- cli formatting, ignored newlines

## [0.14.0]

### Added

- Snippets for SPARQL 1.1 Update

### Changed

- cli format api: when path is omited, use stdin stdout

## [0.13.4]

### Fixed

- handle subject completion request gracefully
- fix formatting for codepoints with width 2 (emojis)
- fix subselect code action when emojis are present

### Added

- tracing capability

## [0.13.3]

### Added

- diagnostic and code-action for same subject triples

### Changed

- prefill order completions

## [0.13.2]

### Added

- add order-condition completions
- prepend variable completions to spo completions

### Changed

- add to result code-action: insert before aggregates
- filter & filter-lang code-action: insert after Dot

## [0.13.1]

### Added

- add new code-action "transform into subselect"

## [0.13.0]

### Added

- object variable replacements

## [0.12.3]

### Fixed

- localize blank-node-property in anon

## [0.12.2]

### Added

- New code-actions: add aggregates to result

## [0.12.1]

### Changed

- set default settings 'remove_unused' to false

### Added

- vim mode for demo
- Lang-Filter code action for objects

### Fixed

- prefix expansion filter

## [0.12.0]

### Fixed

- some typos: also in settings

## [0.11.0]

### Added

- custom capability: get default settings
- custom capability: change settings

## [0.10.0]

### Added

- custom capability: determine what type of operation is present

### Changed

- when typing a prefix and a ":", completion now works

## [0.9.1]

### Fixed

- deduplicate automatic prefix declaration

## [0.9.0]

### Added

- automatically declare and undeclare prefixes

### Fixed

- completion localization after "a"

## [0.8.0]

### Added

- jump to previous important position

### Fixed

- when jumping to the end of the top ggp and its not empty the formatting is now fixed

## [0.7.2]

### Added

- diagnostic: when group by is used: are the selected variables in the group by clause?
- diagnostic: when a variable is assigned in the select clause, was it already defined?

### Fixed

- property list completion context

## [0.7.1]

### Changed

- property list is not part of the global completion context

## [0.7.0]

### Changed

- replace tree-sitter with hand-written parser
  - this effects almost everything and behaviour changes are possible
- **breaking** identify operation type always returns a String
- update various dependencies

### Fixed

- syntax highlighting of comments in demo editor
- tokenize 'DELETE WHERE'
- tokenize comments

## [0.6.4]

### Added

- context sensitivity for completions

### Changed

- Jump location after solution modifiers

### Fixed

- localization for inverse path completions

## [0.6.3]

### Added

- online completion support for bin target

## [0.6.2]

### Changed

- updated and removed various dependencies

## [0.6.1]

### Fixed

- bug in formatter

## [0.6.0]

### Added

- configurable completion query timeout
- configurable completion query result limit
- development setup documentation
- debug log for completion queries
- semantic variable completions: hasHeight -> ?height
- async processing of long running requests (completion and ping)
- custom lsp message "jump", to jump to next relevant location

### Changed

- backends configuration in demo editor is now yaml not json
- completion details are in completion item label_details instead of detail 
  (gets always rendered in monaco, not just when hovering)


### Fixed

- langtag tokenization
- prefix-compression in service blocks
- variable completions
- textual rendering of rdf-terms
- various completion query templates


## [0.5.6] - 2025-04-01

### Fixed

- formatting comments with correct indentation


## [0.5.3] - 2025-03-15

### Fixed

- formatting construct where queries

## [0.5.2] - 2025-03-15

### Added

- sub select snippet
- code action: filter variable
- quickfix for "unused-prefix"

### Fixed

- add to result for vars in sub select binding

## [0.5.1] - 2025-03-15

### Fixed

- tokenize PNAME_LN

### Added

- code action: add variable to result

## [0.5.0] - 2025-03-15

### Added

- ll parser
- cursor localization for completion
- completions for select bindings
- completions for solution modifiers

## [0.4.0] - 2025-02-25

### Added

- function to determine type (Query or Update)

## [0.3.5] - 2025-02-16

### Fixed

- formatting distinct keyword in aggregate
- formatting modify
- formatting describe

## [0.3.4] - 2025-02-03

### Added

- formatting support for any utf-8 input

## [0.3.3] - 2025-02-02

### Fixed

- Fixed bugs in formatter

## [0.3.2] - 2025-01-31

### Added

- stability test for formatter

### Fixed

- fixed typo in diagnostic
- reimplemented formatting options for new formatting algorithm

## [0.3.1] - 2025-01-30

### Added

- formatting inline format statements

### Fixed

- formatting input with comments at any location

## [0.3.0] - 2025-01-20

### Added

- new format option "check": dont write anything, just check if it would

## [0.2.4] - 2025-01-20

### Fixed

- add trailing newline when formatting with format cli subcommand

## [0.2.3] - 2025-01-12

### Fixed

- positions are (by default) utf-16 based, i changed the implementation to respect this

## [0.2.2] - 2025-01-09

### Fixed

- handle textdocuments-edits with utf-8 characters

## [0.2.1] - 2025-01-09

### Fixed

- formatting strings with commas

## [0.2.0] - 2025-01-09

### Added

- new code-action: declare prefix
- example for monaco-editor with a language-client attached to this language-server
- formatter subcommand uses user-configuration
- this CHANGELOG

### Fixed

- format subcommand writeback-bug
- formatting of Blank and ANON nodes

### Changed

- format cli subcommand: --writeback option, prints to stdout by default
