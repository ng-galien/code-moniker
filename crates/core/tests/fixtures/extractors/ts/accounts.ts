// User accounts module.
//
// HTTP layer + service factory for the user resource. The repository is
// passed in by the composition root (see app/bootstrap.ts) so this module
// stays free of any I/O concern of its own.

import { Request, Response, NextFunction } from "express";
import { z } from "zod";
import { UserRepository, type UserRow } from "./repository";

/**
 * Validation schema for the POST /users payload.
 *
 * Tags default to the empty list so callers can omit the field entirely.
 * Email format is enforced server-side because the client cannot be trusted
 * (see incident postmortem in docs/incidents/2024-08-bad-emails.md).
 */
const CreateUserSchema = z.object({
	email: z.string().email(),
	name: z.string().min(1).max(100),
	tags: z.array(z.string()).default([]),
});

export type CreateUserInput = z.infer<typeof CreateUserSchema>;

/** Surfaced to the client as 422 with the offending fields. */
export class ValidationError extends Error {
	constructor(public readonly fields: Record<string, string>) {
		super("validation failed");
		this.name = "ValidationError";
	}
}

export interface UserService {
	create(input: CreateUserInput): Promise<UserRow>;
	findById(id: string): Promise<UserRow | null>;
	withTag(tag: string): AsyncIterable<UserRow>;
}

export class UserController {
	// Service is injected — avoids the controller knowing about persistence.
	constructor(private readonly service: UserService) {}

	/** POST /users */
	async create(req: Request, res: Response, next: NextFunction): Promise<void> {
		try {
			const parsed = CreateUserSchema.safeParse(req.body);
			if (!parsed.success) {
				// Flatten the zod issue tree into a flat field → message map
				// because the frontend form library expects that shape.
				const fields = Object.fromEntries(
					parsed.error.issues.map((i) => [i.path.join("."), i.message]),
				);
				throw new ValidationError(fields);
			}
			const user = await this.service.create(parsed.data);
			res.status(201).json(user);
		} catch (err) {
			next(err);
		}
	}

	/** GET /users/:id */
	async get(req: Request, res: Response, next: NextFunction): Promise<void> {
		const id = req.params.id;
		if (!id) {
			res.status(400).json({ error: "missing id" });
			return;
		}
		try {
			const user = await this.service.findById(id);
			if (!user) {
				res.status(404).end();
				return;
			}
			res.json(user);
		} catch (err) {
			next(err);
		}
	}

	// TODO(perf): paginate — this buffers the whole tag set in memory.
	async listByTag(req: Request, res: Response): Promise<void> {
		const tag = String(req.query.tag ?? "");
		const out: UserRow[] = [];
		for await (const u of this.service.withTag(tag)) {
			out.push(u);
		}
		res.json(out);
	}
}

/**
 * Composition root for the service. The returned object captures `repo`
 * in a closure so consumers don't need to thread it themselves.
 */
export function buildService(repo: UserRepository): UserService {
	return {
		async create(input) {
			// Email uniqueness is enforced here, not at the DB layer, because
			// the FK error would surface as a generic 500 to the client.
			const existing = await repo.findByEmail(input.email);
			if (existing) {
				throw new ValidationError({ email: "already exists" });
			}
			return repo.insert({ ...input, id: makeId(input.email) });
		},
		async findById(id) {
			return repo.findById(id);
		},
		async *withTag(tag) {
			// Streaming generator — the controller decides how many to consume.
			for await (const u of repo.scan()) {
				if (u.tags.includes(tag)) {
					yield u;
				}
			}
		},
	};
}

// Local-part of the email, lowercased. Falls back to the full address when
// no `@` is present (defensive — the schema rejects that input upstream).
function makeId(email: string): string {
	const at = email.indexOf("@");
	return at > 0 ? email.slice(0, at).toLowerCase() : email.toLowerCase();
}
