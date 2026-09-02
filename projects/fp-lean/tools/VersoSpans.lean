import Verso.Doc.Concrete

open Lean Parser Verso
open scoped Lean.Doc.Syntax

structure ExportSpan where
  start : Nat
  stop : Nat
  kind : String
  level : Option Nat := none
deriving ToJson

structure ExportDocument where
  path : String
  sourceHash : String
  spans : Array ExportSpan
deriving ToJson

structure ExportManifest where
  schema : Nat := 2
  generator : String := "Verso.Parser.document"
  versoRevision : String
  /-- The Lake manifest that pins `versoRevision`, relative to this file's
  directory. Documents outside that Lake package (the example modules whose
  equational-step justifications the book renders) are validated against it. -/
  lakeManifest : String
  documents : Array ExportDocument
deriving ToJson

structure DocEnvelope where
  titleStart : Nat
  titleEnd : Nat
  bodyStart : Nat

private def findEnvelope (source : String) : Except String (Option DocEnvelope) := do
  let bytes := source.toUTF8
  let mut offset := 0
  for line in source.splitOn "\n" do
    let indent := line.utf8ByteSize - (line.dropWhile Char.isWhitespace).utf8ByteSize
    let trimmed := line.dropWhile Char.isWhitespace
    if trimmed.startsWith "#doc " then
      let lineBytes := line.toUTF8
      let mut openQuote : Option Nat := none
      let mut closeQuote : Option Nat := none
      let mut escaped := false
      for i in [:lineBytes.size] do
        let b := lineBytes[i]!
        if let none := openQuote then
          if b == 34 then openQuote := some i
        else if escaped then
          escaped := false
        else if b == 92 then
          escaped := true
        else if b == 34 then
          closeQuote := some i
          break
      let some openPos := openQuote
        | throw s!"#doc title has no opening quote at byte {offset + indent}"
      let some closePos := closeQuote
        | throw s!"#doc title has no closing quote at byte {offset + indent}"
      let lineEnd := offset + line.utf8ByteSize
      let bodyStart := lineEnd + if lineEnd < bytes.size then 1 else 0
      return some {
        titleStart := offset + openPos + 1
        titleEnd := offset + closePos
        bodyStart
      }
    offset := offset + line.utf8ByteSize + 1
  return none

private def fnv1a64 (source : String) : UInt64 := Id.run do
  let mut hash : UInt64 := 14695981039346656037
  for byte in source.toUTF8 do
    hash := (hash ^^^ byte.toUInt64) * 1099511628211
  return hash

private def isInlineKind (kind : Name) : Bool :=
  kind == ``Lean.Doc.Syntax.text ||
  kind == ``Lean.Doc.Syntax.emph ||
  kind == ``Lean.Doc.Syntax.bold ||
  kind == ``Lean.Doc.Syntax.link ||
  kind == ``Lean.Doc.Syntax.image ||
  kind == ``Lean.Doc.Syntax.footnote ||
  kind == ``Lean.Doc.Syntax.linebreak ||
  kind == ``Lean.Doc.Syntax.code ||
  kind == ``Lean.Doc.Syntax.role ||
  kind == ``Lean.Doc.Syntax.inline_math ||
  kind == ``Lean.Doc.Syntax.display_math

private partial def topInlines (stx : Syntax) : Array Syntax := Id.run do
  let mut found := #[]
  for arg in stx.getArgs do
    if isInlineKind arg.getKind then
      found := found.push arg
    else
      found := found ++ topInlines arg
  return found

private partial def carriesProse (source : String) (stx : Syntax) : Bool :=
  if stx.getKind == ``Lean.Doc.Syntax.text then
    match stx.getRange? with
    | some range => (range.start.extract source range.stop).any Char.isAlphanum
    | none => false
  else if stx.getKind == ``Lean.Doc.Syntax.image then
    stx.getArgs.any fun arg =>
      arg.getKind == strLitKind && match arg.getRange? with
        | some range => (range.start.extract source range.stop).any Char.isAlphanum
        | none => false
  else
    stx.getArgs.any (carriesProse source)

private structure BlockContext where
  inList : Bool := false
  inQuote : Bool := false
  inTable : Bool := false

private def BlockContext.kind (ctx : BlockContext) : String :=
  if ctx.inTable then "table"
  else if ctx.inList then "list_item"
  else if ctx.inQuote then "block_quote"
  else "paragraph"

/-- Verso's `linebreak` node is a mid-paragraph source line wrap, not a sentence
boundary (it renders as plain whitespace, not `<br>`); splitting spans on it
used to sever sentences that upstream wrapped across two lines. It is still
excluded from the span's own start/stop: a trailing linebreak (e.g. the line
that ends a table cell right before the next row's `*`) is source structure,
not translatable content, and letting it into the span ate the newline the
structure depends on when the translation was spliced back in. -/
private def inlineSpans (source : String) (base : Nat) (container : Syntax)
    (kind : String) (level : Option Nat := none) : Array ExportSpan := Id.run do
  let content := (topInlines container).filter (·.getKind != ``Lean.Doc.Syntax.linebreak)
  if content.isEmpty || !content.any (carriesProse source) then #[]
  else
    match content[0]!.getRange?, content.back!.getRange? with
    | some first, some last =>
        let raw := first.start.extract source last.stop
        if raw.trimAscii.startsWith "_Copyright " && raw.trimAscii.endsWith "_" then #[]
        else #[{start := base + first.start.byteIdx, stop := base + last.stop.byteIdx, kind, level}]
    | _, _ => #[]

/-! ## Equational-step justifications

FP in Lean's `anchorEqSteps`/`eqSteps` code blocks (`FPLean/Examples.lean`,
`moduleEqSteps`) split the anchored example fragment on the `={` and `}=`
tokens and render every step whose first token is a doc comment as prose: the
comment body (without the comment delimiters, trimmed) is parsed as a Markdown
paragraph whose backtick spans are matched against the code. The text shown in
the book comes from the example module, and the book's code-block payload must
match that fragment line by line (`Verso.ExpectString.expectString`). Both
copies are therefore exported as `doc_comment` spans covering exactly the
trimmed comment body so that identical translations can be spliced into each. -/

private def eqStepsBlockNames : List Name := [`anchorEqSteps, `moduleEqSteps, `eqSteps]

private def isSpaceByte (b : UInt8) : Bool :=
  b == 32 || b == 9 || b == 10 || b == 13

private def byteAt (bytes : ByteArray) (i : Nat) (c : Char) : Bool :=
  i < bytes.size && bytes[i]! == c.toUInt8

/-- Find the closing delimiter of a doc comment whose body starts at `start`,
honouring nested block comments the way Lean's comment lexer does. -/
private def docCommentClose? (bytes : ByteArray) (start stop : Nat) : Option Nat := Id.run do
  let mut k := start
  let mut depth := 1
  while k + 1 < stop do
    if byteAt bytes k '/' && byteAt bytes (k + 1) '-' then
      depth := depth + 1
      k := k + 2
    else if byteAt bytes k '-' && byteAt bytes (k + 1) '/' then
      depth := depth - 1
      if depth == 0 then return some k
      k := k + 2
    else
      k := k + 1
  return none

/-- Doc comments that open an equational step (a doc comment right after `={`)
inside `bytes[lo:stop]`, as spans over the trimmed comment body offset by `base`. -/
private def eqStepDocCommentSpans (bytes : ByteArray) (base lo stop : Nat) : Array ExportSpan := Id.run do
  let mut spans := #[]
  let mut i := lo
  while i + 1 < stop do
    if !(byteAt bytes i '=' && byteAt bytes (i + 1) '{') then
      i := i + 1
      continue
    let mut j := i + 2
    while j < stop && isSpaceByte bytes[j]! do
      j := j + 1
    if !(byteAt bytes j '/' && byteAt bytes (j + 1) '-' && byteAt bytes (j + 2) '-') then
      i := j
      continue
    let bodyStart := j + 3
    match docCommentClose? bytes bodyStart stop with
    | none => i := bodyStart
    | some closePos =>
      let mut s := bodyStart
      while s < closePos && isSpaceByte bytes[s]! do
        s := s + 1
      let mut e := closePos
      while e > s && isSpaceByte bytes[e - 1]! do
        e := e - 1
      if s < e && (String.fromUTF8! (bytes.extract s e)).any Char.isAlphanum then
        spans := spans.push {start := base + s, stop := base + e, kind := "doc_comment"}
      i := closePos + 2
  return spans

private partial def firstIdent? (stx : Syntax) : Option Name :=
  if stx.isIdent then some stx.getId
  else stx.getArgs.findSome? firstIdent?

private partial def firstStrLit? (stx : Syntax) : Option Syntax :=
  if stx.getKind == strLitKind then some stx
  else stx.getArgs.findSome? firstStrLit?

/-- Justification doc comments inside an equational-steps code block. Other
code blocks are preserved byte for byte. -/
private def codeBlockSpans (source : String) (base : Nat) (stx : Syntax) : Array ExportSpan :=
  match firstIdent? stx with
  | some name =>
    if eqStepsBlockNames.contains name then
      let payload := (firstStrLit? stx).getD stx
      match payload.getRange? with
      | some range =>
        eqStepDocCommentSpans source.toUTF8 base range.start.byteIdx range.stop.byteIdx
      | none => #[]
    else #[]
  | none => #[]

private def findBytes (bytes : ByteArray) (pattern : ByteArray) (offset : Nat) : Option Nat := Id.run do
  if pattern.size == 0 then return none
  let mut i := offset
  while i + pattern.size <= bytes.size do
    let mut matched := true
    for k in [:pattern.size] do
      if bytes[i + k]! != pattern[k]! then
        matched := false
        break
    if matched then return some i
    i := i + 1
  return none

/-- Justification doc comments in the `equational steps … stop equational
steps` commands of an example module (`ExampleSupport.lean`'s `eqSteps`
syntax). These are the fragments `anchorEqSteps` blocks display. -/
private def exampleModuleSpans (source : String) : Array ExportSpan := Id.run do
  let bytes := source.toUTF8
  let opener := "equational steps".toUTF8
  let closer := "stop equational steps".toUTF8
  let mut spans := #[]
  let mut cursor := 0
  while true do
    let some start := findBytes bytes opener cursor | break
    let regionStart := start + opener.size
    let regionStop := (findBytes bytes closer regionStart).getD bytes.size
    spans := spans ++ eqStepDocCommentSpans bytes 0 regionStart regionStop
    cursor := regionStop + closer.size
  return spans

private def headingLevel (source : String) (stx : Syntax) : Option Nat := do
  let range ← stx.getRange?
  let raw := range.start.extract source range.stop
  let level := raw.toList.takeWhile (· == '#') |>.length
  if level > 0 then some level else none

private def directiveName? (stx : Syntax) : Option Name := do
  if stx.getKind != ``Lean.Doc.Syntax.directive then none else
  let ident ← stx.getArgs.find? (·.isIdent)
  some ident.getId

private partial def collectBlocks (source : String) (base : Nat) (stx : Syntax)
    (ctx : BlockContext := {}) : Array ExportSpan := Id.run do
  let kind := stx.getKind
  if kind == ``Lean.Doc.Syntax.para then
    return inlineSpans source base stx ctx.kind
  if kind == ``Lean.Doc.Syntax.header then
    return inlineSpans source base stx "heading" (headingLevel source stx)
  if kind == ``Lean.Doc.Syntax.codeblock then
    return codeBlockSpans source base stx
  if kind == ``Lean.Doc.Syntax.metadata_block ||
      kind == ``Lean.Doc.Syntax.command ||
      kind == ``Lean.Doc.Syntax.link_ref then
    return #[]
  if kind == ``Lean.Doc.Syntax.footnote_ref then
    return inlineSpans source base stx ctx.kind

  let mut here := #[]
  let mut childCtx := ctx
  if kind == ``Lean.Doc.Syntax.ul || kind == ``Lean.Doc.Syntax.ol ||
      kind == ``Lean.Doc.Syntax.dl || kind == ``Lean.Doc.Syntax.li ||
      kind == ``Lean.Doc.Syntax.desc then
    childCtx := {childCtx with inList := true}
  if kind == ``Lean.Doc.Syntax.blockquote then
    childCtx := {childCtx with inQuote := true}
  if kind == ``Lean.Doc.Syntax.directive && directiveName? stx == some `table then
    childCtx := {childCtx with inTable := true}

  if kind == ``Lean.Doc.Syntax.desc then
    -- The first syntax child after ':' contains the term; later children are
    -- the term's definition blocks and are traversed normally below.
    if let some term := stx.getArgs[1]? then
      here := here ++ inlineSpans source base term childCtx.kind

  for arg in stx.getArgs do
    here := here ++ collectBlocks source base arg childCtx
  return here

private def parseDocumentBody (env : Environment) (body fileName : String) : IO (Except String Syntax) := do
  let input := mkInputContext body fileName
  let state := Verso.Parser.document.run
    input {env, options := {}} (getTokenTable env) (mkParserState body)
  if !state.allErrors.isEmpty then
    let errors := state.allErrors.map fun (pos, _, err) =>
      s!"{fileName}:{(input.fileMap.toPosition pos).line}:{(input.fileMap.toPosition pos).column}: {err}"
    return .error (String.intercalate "\n" errors.toList)
  if state.pos < body.rawEndPos && !(state.pos.extract body body.rawEndPos).all Char.isWhitespace then
    return .error s!"{fileName}: official Verso parser left input at byte {state.pos.byteIdx}"
  if state.stxStack.size == 0 then
    return .error s!"{fileName}: official Verso parser produced no syntax"
  return .ok (state.stxStack.get! (state.stxStack.size - 1))

private def exportDocument (env : Environment) (diskPath manifestPath : String) : IO (Except String ExportDocument) := do
  let source ← IO.FS.readFile diskPath
  let envelope ← match findEnvelope source with
    | .ok value => pure value
    | .error message => return .error s!"{manifestPath}: {message}"
  let mut spans := #[]
  if let some envelope := envelope then
    spans := spans.push {
      start := envelope.titleStart
      stop := envelope.titleEnd
      kind := "heading"
      level := some 1
    }
    let body := (String.Pos.Raw.mk envelope.bodyStart).extract source source.rawEndPos
    let parsed ← parseDocumentBody env body manifestPath
    let parsedSyntax ← match parsed with
      | .ok parsedSyntax => pure parsedSyntax
      | .error message => return .error message
    spans := spans ++ collectBlocks body envelope.bodyStart parsedSyntax
  else
    spans := exampleModuleSpans source
  spans := spans.qsort (fun a b => a.start < b.start)
  return .ok {path := manifestPath, sourceHash := toString (fnv1a64 source), spans}

unsafe def main (argsList : List String) : IO UInt32 := do
  let args := argsList.toArray
  if args.size < 6 then
    IO.eprintln "usage: VersoSpans OUTPUT VERSO_REVISION LAKE_MANIFEST SOURCE_ROOT MANIFEST_PREFIX SOURCE..."
    return 2
  let output := args[0]!
  let revision := args[1]!
  let lakeManifest := args[2]!
  let sourceRoot := System.FilePath.mk args[3]!
  let manifestPrefix := System.FilePath.mk args[4]!
  initSearchPath (← findSysroot)
  enableInitializersExecution
  let env ← importModules (loadExts := true) #[{module := `Verso.Doc.Concrete}] {}
  let mut documents := #[]
  for relative in args[5:] do
    let diskPath := sourceRoot / relative
    let manifestPath := manifestPrefix / relative
    match ← exportDocument env diskPath.toString manifestPath.toString with
    | .ok document => documents := documents.push document
    | .error message =>
        IO.eprintln message
        return 1
  let manifest : ExportManifest := {versoRevision := revision, lakeManifest, documents}
  IO.FS.writeFile output (toJson manifest).compress
  return 0
