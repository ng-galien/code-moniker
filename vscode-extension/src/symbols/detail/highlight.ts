import { createHighlighterCore } from "@shikijs/core";
import type { HighlighterCore } from "@shikijs/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";
import c from "@shikijs/langs/c";
import cpp from "@shikijs/langs/cpp";
import csharp from "@shikijs/langs/csharp";
import css from "@shikijs/langs/css";
import go from "@shikijs/langs/go";
import html from "@shikijs/langs/html";
import java from "@shikijs/langs/java";
import javascript from "@shikijs/langs/javascript";
import json from "@shikijs/langs/json";
import jsx from "@shikijs/langs/jsx";
import kotlin from "@shikijs/langs/kotlin";
import markdown from "@shikijs/langs/markdown";
import python from "@shikijs/langs/python";
import rust from "@shikijs/langs/rust";
import shellscript from "@shikijs/langs/shellscript";
import toml from "@shikijs/langs/toml";
import tsx from "@shikijs/langs/tsx";
import typescript from "@shikijs/langs/typescript";
import yaml from "@shikijs/langs/yaml";
import darkPlus from "@shikijs/themes/dark-plus";
import lightPlus from "@shikijs/themes/light-plus";

import { SourceLine, SourceSnippet } from "../../daemon/model";

export interface HighlightedSourceSnippet extends SourceSnippet {
	lines: HighlightedSourceLine[];
}

export interface HighlightedSourceLine extends SourceLine {
	tokens: HighlightedSourceToken[];
}

export interface HighlightedSourceToken {
	text: string;
	darkColor?: string;
	fontStyle?: number;
	lightColor?: string;
}

let highlighterPromise: Promise<HighlighterCore> | undefined;

export async function highlightSource(
	source: SourceSnippet,
	language: string,
): Promise<HighlightedSourceSnippet> {
	const lang = shikiLanguage(language, source.file);
	try {
		const highlighter = await sourceHighlighter();
		const highlighted = highlighter.codeToTokensWithThemes(source.lines.map((line) => line.text).join("\n"), {
			lang,
			themes: {
				dark: "dark-plus",
				light: "light-plus",
			},
		});
		return {
			...source,
			lines: source.lines.map((line, index) => ({
				...line,
				tokens: sourceTokens(line, highlighted[index] ?? []),
			})),
		};
	} catch {
		return {
			...source,
			lines: source.lines.map((line) => ({
				...line,
				tokens: [{ text: line.text || " " }],
			})),
		};
	}
}

function sourceHighlighter(): Promise<HighlighterCore> {
	highlighterPromise ??= createHighlighterCore({
		themes: [darkPlus, lightPlus],
		langs: [
			c,
			cpp,
			csharp,
			css,
			go,
			html,
			java,
			javascript,
			json,
			jsx,
			kotlin,
			markdown,
			python,
			rust,
			shellscript,
			toml,
			tsx,
			typescript,
			yaml,
		],
		engine: createJavaScriptRegexEngine(),
	});
	return highlighterPromise;
}

function sourceTokens(
	line: SourceLine,
	tokens: {
		content: string;
		variants?: {
			dark?: { color?: string; fontStyle?: number };
			light?: { color?: string; fontStyle?: number };
		};
	}[],
): HighlightedSourceToken[] {
	if (tokens.length === 0) {
		return [{ text: line.text || " " }];
	}
	return tokens.map((token) => ({
		text: token.content,
		darkColor: token.variants?.dark?.color,
		fontStyle: token.variants?.light?.fontStyle ?? token.variants?.dark?.fontStyle,
		lightColor: token.variants?.light?.color,
	}));
}

function shikiLanguage(language: string, file: string): string {
	const lang = language.toLowerCase();
	if (lang === "javascriptreact") {
		return "jsx";
	}
	if (lang === "typescriptreact") {
		return "tsx";
	}
	if (lang) {
		return lang;
	}
	const ext = file.split(".").pop()?.toLowerCase();
	if (ext === "js" || ext === "jsx" || ext === "ts" || ext === "tsx") {
		return ext;
	}
	return ext || "text";
}
