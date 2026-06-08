// Small, self-contained code samples per language, used to seed scratch and
// catalog notebooks. They deliberately contain one tidy and one offending
// symbol so common rules light up.

export const SAMPLES: Record<string, string> = {
	rust: "fn tidy() {\n    let _ = 1;\n}\n\nfn DoThing() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    let _ = a + b + c + d + e;\n}\n",
	typescript:
		"function tidy() {\n  return 1;\n}\n\nfunction DoThing() {\n  const a = 1;\n  const b = 2;\n  const c = 3;\n  const d = 4;\n  const e = 5;\n  return a + b + c + d + e;\n}\n",
	python:
		"def tidy():\n    return 1\n\n\ndef DoThing():\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    e = 5\n    return a + b + c + d + e\n",
	go: "package sample\n\nfunc tidy() int {\n\treturn 1\n}\n\nfunc DoThing() int {\n\ta := 1\n\tb := 2\n\tc := 3\n\td := 4\n\te := 5\n\treturn a + b + c + d + e\n}\n",
	java: "class Sample {\n    int tidy() {\n        return 1;\n    }\n\n    int DoThing() {\n        int a = 1;\n        int b = 2;\n        int c = 3;\n        int d = 4;\n        int e = 5;\n        return a + b + c + d + e;\n    }\n}\n",
	csharp:
		"class Sample {\n    int Tidy() {\n        return 1;\n    }\n\n    int do_thing() {\n        int a = 1;\n        int b = 2;\n        int c = 3;\n        int d = 4;\n        int e = 5;\n        return a + b + c + d + e;\n    }\n}\n",
	sql: "CREATE FUNCTION tidy() RETURNS int AS $$\nBEGIN\n    RETURN 1;\nEND;\n$$ LANGUAGE plpgsql;\n",
};

export function sampleText(langId: string): string {
	return SAMPLES[langId] ?? "// sample to test rules against\n";
}
