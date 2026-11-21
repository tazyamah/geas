# From writing `.geas` to showing the specification to a person (geas-develop)

> This page is a translation of the Japanese skill [skills/geas-develop/SKILL.md](../../skills/geas-develop/SKILL.md) (the Japanese text is the source of truth, and the Japanese file is the one an AI actually loads as a skill). Use: after the plan is ready, how to write `.geas`, check it, and show the specification to a person; ask the checker's questions and reviewer-perspective questions one at a time, and show the survivors of the mutation check to the person.

Source: the reviewer-perspective questions and the mutation check follow the idea of GitHub Next's PDD Kit (Proof-Driven Development). The aim, being able to trust AI-written code without a person reading it line by line, is the same as geas's.

From the plan (made with `skills/plan-interview`), write `.geas`, check it, and make the specification the person reviews. **People do not read `.geas`.** What people read is only the generated specification, so before showing it, the AI first removes the places where a person would likely get stuck.

## Promises

- **Ask one question at a time.** Keep it short. Do not show a long list and make them answer everything. Make it a form answerable with either an option (a number) or a free answer, and add one recommendation and its reason (the same as `plan-interview`).
- **Do not decide on your own.** Ask a person about what is not decided, or write it in the plan as "unspecified" (deliberately not decided).
- **Use plain words for the person.** Do not talk about syntax. Ask only about what changes promises (definitions, guarantees, meaning).
- Ask only what **cannot be read as an answer** even by reading the specification (generated from the code). Do not ask what can be read.
- Do not dig up stakeholders who have not appeared in the conversation. What you ask about is the holes in the **behavior** of what is being built now.

## How to proceed

1. **Write the `.geas`**, with [docs/language-reference.md](language-reference.md) and a close example (`tests/fixtures/overseas_shop/`) at hand. Names are ASCII, and the Japanese display name is in the preceding `///`.
2. **Check** (`geas check app.geas`. When the AI is fixing, `--json` is easier to read: each finding carries `hint`, a clue to the section to read and how to fix). Fix errors. **A question (QUESTION) is something the checker found "not decided", so ask the person one at a time.** When you get the answer, reflect it in the code and check again. Ask the next question after that.
3. **Make the specification, and the AI reviews it first** (`geas spec` / `geas review`). Read the specification from the top as a reviewer, and confirm, from the viewpoints below, "can the answer be read from this specification?". Ask the person, **one at a time**, only the viewpoints that cannot be read. Reflect the answer in both the plan (the table of examples) and the code. If deliberately not deciding, write it in the plan's "unspecified".
4. **Run the mutation check** (`geas mutate app.geas`. When updating, `--base-git HEAD~1` for only the changed lines). **Survivors** (promises that the checker said nothing about even when broken) are shown to the person and confirmed one by one. If the plan has examples, add `--run --plan plan.md` and first remove those the examples stop (a build is involved, so it is slow). If it is a promise that should be protected, add it as an invariant, transition or example. If it is an originally redundant check, leave it as it is.
5. **When updating, attach the receipt** (`geas receipt app.geas --base-git HEAD~1 --plan plan.md`). The "places where behavior may change" (deeds, URLs, screens, use cases) are a guide to the range in which the person confirms the behavior. Paste what has weakened guarantees and the added human guarantees at the top of the PR description. The same content appears in the diff HTML (`review --diff-o`).
6. **Right before the PR, `geas bench`** (when there is a case collection). Compare with the previous JSON to see whether the AI's ability to write has not dropped. Run it also after changing `plan-interview` or the language specification.
7. **Hand the specification to the person.** Have them confirm whether they can read the whole and say "this is right". The points raised here are treated as what could not be removed by step 4, and become material for reviewing the viewpoints.

## Reviewer-perspective viewpoints (used in step 3)

Viewpoints tied to how geas works. They are candidates for "if a person reviewed this specification, what would they ask?"; apply them from the top and ask those that cannot be read.

- **Limits and ranges**: what happens when a quantity, amount or count exceeds the upper limit. (the type's range, the `max` of a mapping, the handling of writes the checker cannot show)
- **When they arrive at the same time**: when two people operate the same thing at the same time; when a screen overwrites while stale. (who is responsible for serialization; the version check on writes is undecided)
- **When the outside does not reply**: when payment or the warehouse is slow, does not reply, or replies twice. The deadline. (the trigger's deadline, idempotency keys, the development mock)
- **Cancelling and deleting**: whether one can quit midway. When something is deleted, what happens to the side that refers to it. (the handling of referrers in `delete`, refusal of transitions)
- **Gaps in states**: when a further operation is done from a finished state. (transitions and the reasons for refusal)
- **Money and fractions**: the direction of rounding, orders mixing currencies, multiplication of ratios. (the rounding specification, currency)
- **Absent and empty**: no input, an empty list, not found. (`required`, existence checks)

Some viewpoints overlap with the checker's questions (the direction of rounding, the case where something does not exist, etc.). What the checker already turns into a question is left to the checker and not asked here.

## How to finish

The person has finished reading the specification, there are no errors or questions in the check, and the handling of the mutation check's survivors (added / left as is) is decided. Then proceed to `geas rust`. Do not increase hand-written code (code that does not appear in the plan or the `.geas` should be removed, not added).

## After operation

For incidents or inquiries, pass the generated server's log (the `geas.route` lines on standard error) to `geas trace app.geas log --plan plan.md`. What cannot be tied to the specification's routes, deeds, results and use cases is suspected of being a drift between the code and the specification, so look at it first.
