// A CodeMirror editor for mise.toml: TOML highlighting, and from the schema, hover text for each
// key and value, completion of keys, sections and choices, and problems as you type. It is
// loaded only when a page shows an editor.

import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  type Completion as CodeMirrorCompletion,
  type CompletionContext,
  completionKeymap,
} from "@codemirror/autocomplete";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import {
  bracketMatching,
  HighlightStyle,
  StreamLanguage,
  type StringStream,
  syntaxHighlighting,
} from "@codemirror/language";
import { linter, lintGutter, lintKeymap } from "@codemirror/lint";
import { EditorState, type Extension, type Text } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  hoverTooltip,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { type Analysis, analyze, type Hover } from "./analyze.ts";
import { MiseSchema } from "./schema.ts";

let schema: Promise<MiseSchema> | undefined;

/** The schema, fetched once for every editor on the page. */
export function loadSchema(): Promise<MiseSchema> {
  schema ??= import("../../../schema/mise.json").then(
    (m) => new MiseSchema(m.default),
  );
  return schema;
}

// TOML, token by token: a key until its `=`, then a value, with inline tables and arrays
// tracked so a key inside `{ }` reads as a key.
type ModeState = { string: string | null; key: boolean; stack: ("{" | "[")[] };

const tomlMode = StreamLanguage.define<ModeState>({
  name: "toml",
  startState: () => ({ string: null, key: true, stack: [] }),
  copyState: (s) => ({ string: s.string, key: s.key, stack: [...s.stack] }),
  token(stream: StringStream, state: ModeState): string | null {
    if (state.string) return inString(stream, state);
    if (stream.sol() && state.stack.length === 0) state.key = true;
    if (stream.eatSpace()) return null;
    if (stream.peek() === "#") {
      stream.skipToEnd();
      return "comment";
    }
    if (
      state.key &&
      state.stack.length === 0 &&
      stream.sol() &&
      stream.match(/^\[\[?[^\]]*\]\]?/)
    ) {
      return "heading";
    }
    const quote = stream.match(/^('''|"""|'|")/) as RegExpMatchArray | null;
    if (quote) {
      state.string = quote[0];
      return inString(stream, state);
    }
    if (state.key) {
      if (stream.match(/^[A-Za-z0-9_-]+/)) return "propertyName";
      if (stream.eat(".")) return "punctuation";
      if (stream.eat("=")) {
        state.key = false;
        return "operator";
      }
      if (stream.eat("}")) {
        state.stack.pop();
        state.key = false;
        return "bracket";
      }
      stream.next();
      return null;
    }
    if (stream.eat("{")) {
      state.stack.push("{");
      state.key = true;
      return "bracket";
    }
    if (stream.eat("[")) {
      state.stack.push("[");
      return "bracket";
    }
    if (stream.eat("]") || stream.eat("}")) {
      state.stack.pop();
      return "bracket";
    }
    if (stream.eat(",")) {
      // After a comma in an inline table comes another key.
      if (state.stack[state.stack.length - 1] === "{") state.key = true;
      return "punctuation";
    }
    if (stream.match(/^(true|false)\b/)) return "bool";
    if (
      stream.match(
        /^\d{4}-\d{2}-\d{2}([Tt ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?([Zz]|[+-]\d{2}:\d{2})?)?/,
      )
    ) {
      return "atom";
    }
    if (
      stream.match(
        /^[+-]?(0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+|inf|nan|\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d[\d_]*)?)/,
      )
    ) {
      return "number";
    }
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: "#" } },
  tokenTable: {
    propertyName: tags.propertyName,
    heading: tags.heading,
    bool: tags.bool,
  },
});

function inString(stream: StringStream, state: ModeState): string {
  const close = state.string!;
  while (!stream.eol()) {
    if (stream.match(close)) {
      state.string = null;
      break;
    }
    if (close.startsWith('"') && stream.peek() === "\\") stream.next();
    stream.next();
  }
  // A single-line string ends with its line, closed or not.
  if (close.length === 1) state.string = null;
  return state.key ? "propertyName" : "string";
}

const highlight = HighlightStyle.define([
  { tag: tags.heading, color: "var(--mte-heading)", fontWeight: "600" },
  { tag: tags.propertyName, color: "var(--mte-key)" },
  { tag: tags.string, color: "var(--mte-string)" },
  { tag: [tags.number, tags.bool, tags.atom], color: "var(--mte-literal)" },
  { tag: tags.comment, color: "var(--mte-comment)", fontStyle: "italic" },
  {
    tag: [tags.operator, tags.punctuation, tags.bracket],
    color: "var(--mte-punctuation)",
  },
]);

const theme = EditorView.theme({
  "&": {
    backgroundColor: "var(--vp-code-block-bg)",
    color: "var(--vp-c-text-1)",
    fontSize: "var(--vp-code-font-size, 0.875em)",
    borderRadius: "8px",
  },
  "&.cm-focused": { outline: "1px solid var(--vp-c-brand-1)" },
  ".cm-scroller": {
    fontFamily: "var(--vp-font-family-mono)",
    lineHeight: "1.7",
  },
  ".cm-content": { caretColor: "var(--vp-c-brand-1)", padding: "12px 0" },
  ".cm-gutters": {
    backgroundColor: "var(--vp-code-block-bg)",
    color: "var(--vp-c-text-3)",
    border: "none",
    borderTopLeftRadius: "8px",
    borderBottomLeftRadius: "8px",
  },
  ".cm-activeLine, .cm-activeLineGutter": {
    backgroundColor: "var(--vp-c-default-soft)",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "var(--vp-c-brand-soft) !important",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--vp-c-bg-elv)",
    color: "var(--vp-c-text-1)",
    border: "1px solid var(--vp-c-divider)",
    borderRadius: "8px",
    boxShadow: "var(--vp-shadow-3)",
    fontFamily: "var(--vp-font-family-base)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--vp-c-brand-soft)",
    color: "var(--vp-c-text-1)",
  },
  ".cm-completionDetail": {
    color: "var(--vp-c-text-2)",
    fontStyle: "normal",
    marginLeft: "1em",
  },
  ".cm-completionInfo": {
    maxWidth: "28em",
    padding: "8px 10px",
    lineHeight: "1.5",
  },
  ".cm-diagnostic": {
    fontFamily: "var(--vp-font-family-base)",
    padding: "4px 8px",
  },
  ".cm-lintRange-info": {
    backgroundImage: "none",
    textDecoration: "line-through",
    textDecorationColor: "var(--vp-c-text-3)",
  },
});

// The analysis of each version of the text, made once however many features ask.
const analyses = new WeakMap<Text, Analysis>();
function analysisOf(doc: Text, resolved: MiseSchema): Analysis {
  let analysis = analyses.get(doc);
  if (!analysis) {
    analysis = analyze(doc.toString(), resolved);
    analyses.set(doc, analysis);
  }
  return analysis;
}

/** A key path as TOML writes it, quoting a key that is not bare. */
export function keyPath(path: string[]): string {
  return path
    .map((key) =>
      /^\d+$/.test(key)
        ? `[${key}]`
        : /^[A-Za-z0-9_-]+$/.test(key)
          ? key
          : JSON.stringify(key),
    )
    .join(".")
    .replace(/\.\[/g, "[");
}

// A description's markdown, enough of it for the schema's text: `code` and links.
function markdown(text: string): DocumentFragment {
  const out = document.createDocumentFragment();
  const re = /`([^`]+)`|(https?:\/\/[^\s)]+[^\s).,;])/g;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    out.append(text.slice(at, m.index));
    if (m[1] !== undefined) {
      const code = document.createElement("code");
      code.textContent = m[1];
      out.append(code);
    } else {
      const a = document.createElement("a");
      a.href = m[2];
      a.textContent = m[2];
      a.target = "_blank";
      a.rel = "noreferrer";
      out.append(a);
    }
    at = m.index + m[0].length;
  }
  out.append(text.slice(at));
  return out;
}

function element(
  tag: string,
  className: string,
  ...children: (Node | string)[]
): HTMLElement {
  const el = document.createElement(tag);
  el.className = className;
  el.append(...children);
  return el;
}

/** The hover card for a key or a value. */
function card(hover: Hover, reference: string): HTMLElement {
  const { info, value } = hover;
  const head = element(
    "div",
    "mte-head",
    element("code", "mte-path", keyPath(info.path)),
  );
  const body = element("div", "mte-card", head);
  head.append(element("span", "mte-type", info.type));
  if (info.deprecated)
    head.append(element("span", "mte-badge mte-deprecated", "deprecated"));
  if (info.experimental)
    head.append(element("span", "mte-badge mte-experimental", "experimental"));
  if (info.description)
    body.append(
      element("div", "mte-desc", markdown(info.description.split("\n")[0])),
    );
  const meta: (Node | string)[] = [];
  if (info.choices.length) {
    meta.push("Choices: ");
    info.choices.forEach((choice, i) => {
      if (i) meta.push(", ");
      const code = element(
        "code",
        value && JSON.stringify(value.scalar) === choice ? "mte-chosen" : "",
        choice,
      );
      meta.push(code);
    });
  }
  if (info.default !== undefined) {
    if (meta.length) meta.push(" · ");
    meta.push("Default: ", element("code", "", info.default));
  }
  if (meta.length) body.append(element("div", "mte-meta", ...meta));
  // Settings have a page of their own, whose anchors are the setting's key.
  if (info.path[0] === "settings" && info.path.length > 1) {
    const link = element(
      "a",
      "mte-ref",
      "Settings reference →",
    ) as HTMLAnchorElement;
    link.href = `/configuration/settings.html#${encodeURIComponent(info.path.slice(1).join("."))}`;
    body.append(link);
  } else if (info.reference) {
    const link = element(
      "a",
      "mte-ref",
      "mise.toml reference →",
    ) as HTMLAnchorElement;
    link.href = `${reference}#${encodeURIComponent(info.reference)}`;
    body.append(link);
  }
  return body;
}

export type EditorOptions = {
  doc: string;
  readonly?: boolean;
  /** The URL of the mise.toml reference page, for the link in each hover card. */
  reference?: string;
  onChange?: (doc: string) => void;
};

/** An editor in `parent`. The schema is loaded first, so it may take a moment. */
export async function createEditor(
  parent: HTMLElement,
  options: EditorOptions,
): Promise<EditorView> {
  const resolved = await loadSchema();
  const reference = options.reference ?? "/configuration/reference.html";
  const extensions = [
    lineNumbers(),
    highlightActiveLineGutter(),
    ...(options.readonly ? [] : [history(), closeBrackets()]),
    drawSelection(),
    highlightActiveLine(),
    bracketMatching(),
    tomlMode,
    syntaxHighlighting(highlight),
    theme,
    EditorView.lineWrapping,
    hoverTooltip(
      (view, pos) => {
        const hover = analysisOf(view.state.doc, resolved).hover(pos);
        // A key mise does not read has its problem's tooltip, which says the same.
        if (!hover || !hover.info.known) return null;
        return {
          pos: hover.from,
          end: hover.to,
          above: true,
          create: () => ({ dom: card(hover, reference) }),
        };
      },
      { hoverTime: 250 },
    ),
    ...(options.readonly ? [] : editing(resolved)),
    keymap.of([
      ...closeBracketsKeymap,
      ...defaultKeymap,
      ...historyKeymap,
      ...(options.readonly
        ? []
        : [...completionKeymap, ...lintKeymap, indentWithTab]),
    ]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) options.onChange?.(update.state.doc.toString());
    }),
  ];
  if (options.readonly)
    extensions.push(
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
    );
  return new EditorView({
    parent,
    state: EditorState.create({ doc: options.doc, extensions }),
  });
}

/** What makes an editor editable: problems as you type and completions. A read-only one has neither. */
function editing(resolved: MiseSchema): Extension[] {
  return [
    linter(
      (view) =>
        analysisOf(view.state.doc, resolved)
          .problems()
          .map((p) => ({ ...p, source: "mise" })),
      {
        delay: 250,
      },
    ),
    lintGutter(),
    autocompletion({
      override: [
        (context: CompletionContext) => {
          const result = analysisOf(context.state.doc, resolved).completions(
            context.pos,
          );
          if (!result) return null;
          // Offer as you type a key, and when asked (Ctrl-Space) anywhere.
          if (
            !context.explicit &&
            result.from === context.pos &&
            !/[[{,=]\s*$|^\s*$/.test(lineBefore(context))
          ) {
            return null;
          }
          return {
            from: result.from,
            to: result.to,
            validFor: /^[\w"'.:-]*$/,
            options: result.options.map((o): CodeMirrorCompletion => ({
              label: o.label,
              apply: o.apply,
              detail: o.detail,
              type:
                o.kind === "property"
                  ? "property"
                  : o.kind === "section"
                    ? "class"
                    : "enum",
              info: o.info
                ? () =>
                    element("div", "mte-desc", markdown(o.info!.split("\n")[0]))
                : undefined,
            })),
          };
        },
      ],
    }),
  ];
}

function lineBefore(context: CompletionContext): string {
  const line = context.state.doc.lineAt(context.pos);
  return context.state.sliceDoc(line.from, context.pos);
}
