// Speed updating, in a browser.
//
// One page with four screens: the one that opens a room, the
// facilitator's, the one that joins, and the player's, which is almost
// always a phone.

import {
  api,
  byId,
  countdown,
  el,
  fail,
  hide,
  joinLink,
  onClick,
  onSubmit,
  plural,
  qrInto,
  remember,
  remembered,
  route,
  show,
  text,
  watch,
} from "./rooms";

type Mode = "bingo" | "open";
type Scale = "written" | "agree" | "chance";
type Phase = "answering" | "talking" | "updating" | "finished";

interface Statement {
  id: number;
  text: string;
  topic: string;
  chance: boolean;
}

interface TopicChoice {
  key: string;
  name: string;
  size: number;
  chosen: boolean;
}

interface Said {
  name: string;
  before: number;
  after: number | null;
}

interface GroupView {
  number: number;
  statement: Statement;
  people: Said[];
  gap: number;
}

interface RoundView {
  number: number;
  groups: GroupView[];
  waiting: string[];
}

interface Persuaded {
  points: number;
  people: number;
}

interface Leader extends Persuaded {
  name: string;
}

interface Who {
  name: string;
  persuaded: Persuaded;
  answered: number;
  of: number;
  updated: number;
  moved: number;
  lines: number;
}

interface Tally {
  statement: Statement;
  n: number;
  mean: number;
  spread: number;
  first_mean: number;
  updated: number;
}

interface HostView {
  join: string;
  mode: Mode;
  scale: Scale;
  topics: TopicChoice[];
  custom: string;
  side: number;
  sides: number[];
  minutes: number;
  lengths: number[];
  deck: number;
  locked: boolean;
  phase: Phase;
  ends: number;
  round: RoundView | null;
  rounds: number;
  players: Who[];
  contested: Tally[];
  shifted: Tally[];
  answers: number;
  updates: number;
  leaders: Leader[];
}

interface Square {
  statement: Statement;
  answer: number | null;
  first: number | null;
  marked: boolean;
}

interface Partner {
  name: string;
  answer: number;
  after: number | null;
}

interface Mine {
  number: number;
  statement: Statement;
  partners: Partner[];
  before: number;
  after: number | null;
}

interface Opposite {
  name: string;
  statement: Statement;
  mine: number;
  theirs: number;
  gap: number;
}

interface PlayerView {
  name: string;
  mode: Mode;
  side: number;
  phase: Phase;
  ends: number;
  round: number;
  squares: Square[];
  topics: [string, string][];
  group: Mine | null;
  waiting: boolean;
  opposites: Opposite[];
  lines: number;
  updated: number;
  moved: number;
  players: number;
  persuaded: Persuaded;
  rank: number | null;
  winners: Leader[];
}

const PHASES: Record<Phase, string> = {
  answering: "Answering",
  talking: "Talking",
  updating: "Where do you stand now?",
  finished: "Finished",
};

/** An answer in words: "+60, agree" or "35%". */
function said(statement: Statement, value: number | null): string {
  if (value === null) return "not answered";
  if (statement.chance) return `${value}%`;
  const sign = value > 0 ? "+" : value < 0 ? "−" : "";
  const size = Math.abs(value);
  const word =
    value === 0
      ? "neutral"
      : size >= 70
        ? value > 0
          ? "strongly agree"
          : "strongly disagree"
        : size >= 30
          ? value > 0
            ? "agree"
            : "disagree"
          : value > 0
            ? "lean agree"
            : "lean disagree";
  return `${sign}${size}, ${word}`;
}

/** Where an answer sits between its two ends, 0 to 1, for colouring. */
function lean(statement: Statement, value: number): number {
  return statement.chance ? value / 100 : (value + 100) / 200;
}

/** Runs a countdown off the server's clock into every element given. */
function ticking(now: () => number, ends: () => number | null, into: () => HTMLElement[]): void {
  setInterval(() => {
    const end = ends();
    for (const node of into()) {
      node.textContent = end === null ? "" : countdown(end - now());
    }
  }, 250);
}

/** A suggestion form, which anybody can use and nobody reads back. */
function suggestions(form: string, field: string, chance: string, done: string): void {
  onSubmit(form, async () => {
    const input = byId<HTMLInputElement>(field);
    await api("POST", "/api/suggest", {
      text: input.value,
      chance: byId<HTMLInputElement>(chance).checked,
    });
    input.value = "";
    byId<HTMLInputElement>(chance).checked = false;
    hide(done, false);
    fail("");
  });
}

/** Pressed-state buttons, one of which is chosen. */
function chips<T>(
  id: string,
  options: { value: T; label: string; on: boolean }[],
  pick: (value: T) => void,
  disabled = false,
): void {
  const box = byId(id);
  box.replaceChildren(
    ...options.map((o) => {
      const b = el("button", { type: "button", className: "chip", disabled }, o.label);
      b.setAttribute("aria-pressed", String(o.on));
      b.addEventListener("click", () => pick(o.value));
      return b;
    }),
  );
}

// ---------------------------------------------------------------- start

function start(): void {
  show("start");
  onClick("open", async () => {
    const keys = await api<{ host: string; join: string }>("POST", "/api/rooms");
    location.href = `/h/${keys.host}`;
  });
  suggestions("suggest-start", "idea-start", "idea-start-chance", "idea-start-done");
}

// ----------------------------------------------------------------- host

function host(key: string): void {
  show("host");
  const at = (path: string): string => `/api/h/${key}${path}`;
  let current: HostView | null = null;
  /** The setup as the facilitator is editing it, before it is sent. */
  let draft: { mode: Mode; scale: Scale; topics: string[]; side: number } | null = null;
  let qrFor = "";

  const send = async (method: string, path: string, body?: unknown): Promise<void> => {
    const result = await api<{ now: number; view: HostView }>(method, at(path), body);
    feed.accept(result);
    fail("");
  };

  const drawSetup = (view: HostView): void => {
    hide("setup", view.locked);
    hide("locked", !view.locked);
    const chosen = view.topics.filter((t) => t.chosen).map((t) => t.name);
    text(
      "locked-note",
      `${view.mode === "bingo" ? `A ${view.side}×${view.side} bingo card each` : "Answer anything"}, from ${plural(view.deck, "statement", "statements")}: ${chosen.join(", ") || "your own"}. People have started answering, so this stays as it is.`,
    );
    if (view.locked) return;
    draft ??= {
      mode: view.mode,
      scale: view.scale,
      topics: view.topics.filter((t) => t.chosen).map((t) => t.key),
      side: view.side,
    };
    const d = draft;
    chips<Mode>(
      "modes",
      [
        { value: "bingo", label: "Bingo card", on: d.mode === "bingo" },
        { value: "open", label: "Answer anything", on: d.mode === "open" },
      ],
      (mode) => {
        d.mode = mode;
        drawSetup(view);
      },
    );
    text(
      "mode-note",
      d.mode === "bingo"
        ? "Everyone is dealt a grid from the topics below. What they talk about is marked; a full line is a bingo."
        : "Everyone sees every statement from the topics below and answers as many as they like.",
    );
    chips<Scale>(
      "scales",
      [
        { value: "written", label: "As written", on: d.scale === "written" },
        { value: "agree", label: "All agree / disagree", on: d.scale === "agree" },
        { value: "chance", label: "All probabilities", on: d.scale === "chance" },
      ],
      (scale) => {
        d.scale = scale;
        drawSetup(view);
      },
    );
    text(
      "scale-note",
      d.scale === "written"
        ? "Most statements are agree or disagree, from −100 to 100; questions starting “What is the chance” ask for a probability."
        : d.scale === "agree"
          ? "Every statement is agree or disagree. Probability questions become claims: “A fish feels pain.”"
          : "Every statement is answered with the chance that it is true, from 0 to 100%.",
    );
    chips<string>(
      "topics",
      view.topics.map((t) => ({
        value: t.key,
        label: `${t.name} (${t.size})`,
        on: d.topics.includes(t.key),
      })),
      (topic) => {
        d.topics = d.topics.includes(topic)
          ? d.topics.filter((k) => k !== topic)
          : [...d.topics, topic];
        drawSetup(view);
      },
    );
    hide("sides-set", d.mode !== "bingo");
    chips<number>(
      "sides",
      view.sides.map((s) => ({ value: s, label: `${s}×${s}`, on: d.side === s })),
      (side) => {
        d.side = side;
        drawSetup(view);
      },
    );
    const custom = byId<HTMLTextAreaElement>("custom");
    if (document.activeElement !== custom && custom.dataset.sent !== view.custom) {
      custom.value = view.custom;
      custom.dataset.sent = view.custom;
    }
    text(
      "setup-note",
      `In use now: ${view.mode === "bingo" ? `${view.side}×${view.side} cards` : "answer anything"}, ${plural(view.deck, "statement", "statements")}. This can change until somebody answers.`,
    );
  };

  onSubmit("setup", async () => {
    if (!draft) return;
    await send("PUT", "/setup", {
      ...draft,
      custom: byId<HTMLTextAreaElement>("custom").value,
    });
  });

  const gapNote = (g: GroupView): string =>
    g.statement.chance ? `${g.gap / 2} points apart` : `${g.gap} points apart`;

  const drawGroups = (view: HostView): void => {
    const box = byId("groups");
    if (!view.round) {
      box.replaceChildren();
      return;
    }
    const rows = view.round.groups.map((g) =>
      el(
        "div",
        { className: "group" },
        el("p", { className: "eyebrow" }, `Pair ${g.number} · ${gapNote(g)}`),
        el("p", { className: "statement" }, g.statement.text),
        el(
          "ul",
          { className: "people" },
          ...g.people.map((p) =>
            el(
              "li",
              {},
              el("b", {}, p.name),
              ` ${said(g.statement, p.before)}`,
              p.after !== null && p.after !== p.before
                ? el("span", { className: "moved" }, ` → ${said(g.statement, p.after)}`)
                : p.after !== null
                  ? el("span", { className: "note" }, " · unchanged")
                  : null,
            ),
          ),
        ),
      ),
    );
    const waiting = view.round.waiting.length
      ? el(
          "p",
          { className: "note" },
          `Not paired: ${view.round.waiting.join(", ")}. They need to answer statements somebody else has answered.`,
        )
      : null;
    box.replaceChildren(el("h2", {}, `Round ${view.round.number}`), ...rows, waiting ?? "");
  };

  const tallyList = (title: string, tallies: Tally[], line: (t: Tally) => string): HTMLElement | null =>
    tallies.length
      ? el(
          "section",
          { className: "panel" },
          el("h2", {}, title),
          el(
            "ol",
            { className: "tallies" },
            ...tallies.map((t) =>
              el("li", {}, el("span", {}, t.statement.text), el("small", {}, line(t))),
            ),
          ),
        )
      : null;

  const round1 = (v: number): string => String(Math.round(v));

  const drawBoard = (view: HostView): void => {
    const people = el(
      "section",
      { className: "panel" },
      el("h2", {}, "People"),
      el(
        "p",
        { className: "note" },
        `${plural(view.answers, "answer", "answers")} so far, ${plural(view.updates, "update", "updates")} of ten points or more.`,
      ),
      el(
        "div",
        { className: "scroll" },
        el(
          "table",
          {},
          el(
            "thead",
            {},
            el(
              "tr",
              {},
              el("th", {}, "Name"),
              el("th", {}, "Answered"),
              el("th", {}, "Updated"),
              el("th", {}, "Moved others"),
              view.mode === "bingo" ? el("th", {}, "Lines") : null,
            ),
          ),
          el(
            "tbody",
            {},
            ...view.players.map((p) =>
              el(
                "tr",
                {},
                el("td", {}, p.name),
                el("td", {}, `${p.answered} of ${p.of}`),
                el("td", {}, String(p.updated)),
                el("td", {}, String(p.persuaded.points)),
                view.mode === "bingo" ? el("td", {}, p.lines ? `${p.lines} 🎉` : "0") : null,
              ),
            ),
          ),
        ),
      ),
    );
    const scale = (t: Tally): string =>
      t.statement.chance ? `${round1(t.mean)}% on average` : `average ${said(t.statement, Math.round(t.mean))}`;
    const board = view.leaders.length
      ? el(
          "section",
          { className: "panel" },
          el("h2", {}, view.phase === "finished" ? "Who changed the most minds" : "Changing minds so far"),
          el(
            "p",
            { className: "note" },
            "Points are how far partners moved towards somebody's answer after talking to them. Moving yourself scores nothing for you.",
          ),
          el(
            "ol",
            { className: "tallies leaders" },
            ...view.leaders.slice(0, 10).map((l, i) =>
              el(
                "li",
                { className: i === 0 ? "first" : "" },
                el("span", {}, el("b", {}, l.name), i === 0 && view.phase === "finished" ? " 🏆" : ""),
                el("small", {}, `${l.points} points, ${plural(l.people, "person", "people")} moved`),
              ),
            ),
          ),
        )
      : null;
    byId("board").replaceChildren(
      ...[
        board,
        tallyList("Where the room is most split", view.contested, (t) =>
          `${plural(t.n, "answer", "answers")}, ${scale(t)}, spread ${round1(t.spread)}`,
        ),
        tallyList("What people changed their minds on", view.shifted, (t) =>
          `${plural(t.updated, "person", "people")} moved; the average went from ${
            t.statement.chance ? `${round1(t.first_mean)}%` : round1(t.first_mean)
          } to ${t.statement.chance ? `${round1(t.mean)}%` : round1(t.mean)}`,
        ),
        view.players.length ? people : null,
      ].filter((n): n is HTMLElement => n !== null),
    );
  };

  const draw = (view: HostView): void => {
    current = view;
    const link = joinLink(view.join);
    const a = byId<HTMLAnchorElement>("join-link");
    a.href = link;
    a.textContent = link;
    if (qrFor !== link) {
      qrFor = link;
      qrInto("join-qr", link);
    }
    byId("room").classList.toggle("tight", view.phase !== "answering");
    text(
      "who",
      view.players.length
        ? `${plural(view.players.length, "person is", "people are")} in: ${view.players.map((p) => p.name).join(", ")}.`
        : "Nobody has joined yet.",
    );
    text("where", view.rounds ? `Round ${view.rounds}` : "");
    drawSetup(view);

    text("h-phase", PHASES[view.phase]);
    chips<number>(
      "lengths",
      view.lengths.map((m) => ({ value: m, label: `${m}`, on: m === view.minutes })),
      (minutes) => void send("PUT", "/minutes", { minutes }).catch((e: unknown) => fail(String(e))),
    );
    const talking = view.phase === "talking";
    byId("start").textContent = view.rounds ? `Start round ${view.rounds + 1}` : "Start the first round";
    hide("start", talking || view.phase === "finished");
    hide("stop", !talking);
    hide("extend", !(talking || (view.phase === "updating" && view.rounds > 0)));
    hide("finish", view.phase === "finished");
    hide("reopen", view.phase !== "finished");
    hide("h-clock", !talking);
    drawGroups(view);
    drawBoard(view);
  };

  onClick("start", () => send("POST", "/round"));
  onClick("stop", () => send("POST", "/stop"));
  onClick("extend", () => send("POST", "/extend"));
  onClick("finish", () => send("POST", "/finish"));
  onClick("reopen", () => send("POST", "/reopen"));

  const feed = watch<HostView>(at(""), draw);
  ticking(
    feed.now,
    () => (current && current.phase === "talking" ? current.ends : null),
    () => [byId("h-clock")],
  );
}

// ----------------------------------------------------------------- join

function join(key: string): void {
  const known = remembered("updating", key);
  if (known) {
    location.replace(`/p/${known}`);
    return;
  }
  show("join");
  api<{ players: number }>("GET", `/api/j/${key}`)
    .then((r) => text("join-count", r.players ? `${plural(r.players, "person is", "people are")} in already.` : ""))
    .catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  onSubmit("name-form", async () => {
    const r = await api<{ player: string }>("POST", `/api/j/${key}`, {
      name: byId<HTMLInputElement>("name").value,
    });
    remember("updating", key, r.player);
    location.href = `/p/${r.player}`;
  });
}

// --------------------------------------------------------------- player

/** One statement's row: the text, a slider, and what it says. */
interface Row {
  box: HTMLElement;
  slider: HTMLInputElement;
  value: HTMLElement;
  mark: HTMLButtonElement | null;
  statement: Statement;
}

function player(key: string): void {
  show("player");
  const at = (path: string): string => `/api/p/${key}${path}`;
  let current: PlayerView | null = null;
  const rows = new Map<number, Row>();
  let builtFor = "";
  /** A slider being dragged is not moved by an update arriving. */
  let holding: HTMLInputElement | null = null;

  const accept = (r: { now: number; view: PlayerView }): void => {
    feed.accept(r);
    fail("");
  };

  const answer = async (id: number, value: number | null): Promise<void> => {
    accept(await api("PUT", at("/answer"), { id, value }));
  };

  const markIt = async (id: number, marked: boolean): Promise<void> => {
    accept(await api("PUT", at("/mark"), { id, marked }));
  };

  const run = (p: Promise<void>): void => {
    p.catch((e: unknown) => fail(e instanceof Error ? e.message : String(e)));
  };

  /** A slider for one statement, sending when let go. */
  const slider = (s: Statement, onSend: (v: number) => void, label: HTMLElement): HTMLInputElement => {
    const input = el("input", {
      type: "range",
      min: s.chance ? "0" : "-100",
      max: "100",
      step: s.chance ? "1" : "5",
      className: "scale unset",
    });
    input.setAttribute("aria-label", s.text);
    input.addEventListener("pointerdown", () => {
      holding = input;
    });
    input.addEventListener("input", () => {
      input.classList.remove("unset");
      label.textContent = said(s, Number(input.value));
    });
    input.addEventListener("change", () => {
      holding = null;
      onSend(Number(input.value));
    });
    return input;
  };

  const ends = (s: Statement): HTMLElement =>
    el(
      "div",
      { className: "ends" },
      el("span", {}, s.chance ? "0%" : "Disagree"),
      el("span", {}, s.chance ? "100%" : "Agree"),
    );

  /** Builds the list once per deck; later views only set its values. */
  const build = (view: PlayerView): void => {
    rows.clear();
    const names = new Map(view.topics);
    const make = (sq: Square): HTMLElement => {
      const s = sq.statement;
      const value = el("span", { className: "value" }, said(s, sq.answer));
      const input = slider(s, (v) => run(answer(s.id, v)), value);
      const clear = el("button", { type: "button", className: "chip small" }, "Clear");
      clear.addEventListener("click", () => run(answer(s.id, null)));
      let mark: HTMLButtonElement | null = null;
      if (view.mode === "bingo") {
        mark = el("button", { type: "button", className: "chip small" }, "Talked about it");
        const m = mark;
        m.addEventListener("click", () => run(markIt(s.id, m.getAttribute("aria-pressed") !== "true")));
      }
      const box = el(
        "div",
        { className: "statement-row", id: `s-${s.id}` },
        el("p", { className: "statement" }, s.text),
        el(
          "p",
          { className: "note" },
          `${names.get(s.topic) ?? s.topic}${
            s.chance ? (s.text.endsWith("?") ? " · a probability" : " · how likely is this to be true?") : ""
          }`,
        ),
        input,
        ends(s),
        el("div", { className: "row-foot" }, value, el("span", {}, mark ?? "", clear)),
      );
      rows.set(s.id, { box, slider: input, value, mark, statement: s });
      return box;
    };

    const list = byId("statements");
    if (view.mode === "bingo") {
      list.replaceChildren(
        el(
          "section",
          { className: "panel" },
          el("h2", {}, "Where do you stand?"),
          el("p", { className: "note" }, "Drag each slider. You can change an answer whenever you like."),
          ...view.squares.map(make),
        ),
      );
      return;
    }
    // In the open mode, one folding section per topic.
    const byTopic = new Map<string, Square[]>();
    for (const sq of view.squares) {
      const list = byTopic.get(sq.statement.topic) ?? [];
      list.push(sq);
      byTopic.set(sq.statement.topic, list);
    }
    list.replaceChildren(
      el(
        "p",
        { className: "note intro" },
        "Answer as many as you like. The more you answer, the better the match.",
      ),
      ...[...byTopic.entries()].map(([topic, squares], i) => {
        const d = el(
          "details",
          { className: "panel topic", open: i === 0 },
          el("summary", {}, el("span", {}, names.get(topic) ?? topic), el("small", { id: `count-${topic}` })),
          ...squares.map(make),
        );
        return d;
      }),
    );
  };

  const drawGrid = (view: PlayerView): void => {
    hide("card-panel", view.mode !== "bingo");
    if (view.mode !== "bingo") return;
    const grid = byId("grid");
    grid.style.gridTemplateColumns = `repeat(${view.side}, 1fr)`;
    grid.replaceChildren(
      ...view.squares.map((sq, i) => {
        const cell = el(
          "button",
          { type: "button", className: `cell${sq.marked ? " marked" : ""}${sq.answer === null ? " empty" : ""}` },
          el("span", { className: "n" }, String(i + 1)),
          el("span", { className: "t" }, sq.statement.text),
        );
        if (sq.answer !== null) {
          cell.style.setProperty("--lean", String(lean(sq.statement, sq.answer)));
        }
        cell.addEventListener("click", () =>
          document.getElementById(`s-${sq.statement.id}`)?.scrollIntoView({ behavior: "smooth", block: "center" }),
        );
        return cell;
      }),
    );
    const answered = view.squares.filter((s) => s.answer !== null).length;
    text(
      "card-note",
      view.lines
        ? `Bingo! ${plural(view.lines, "line", "lines")}. ${answered} of ${view.squares.length} answered.`
        : `${answered} of ${view.squares.length} answered. Talk about a full line for a bingo.`,
    );
  };

  const drawNow = (view: PlayerView): void => {
    const box = byId("now");
    const g = view.group;
    if (view.phase === "talking" && g) {
      const s = g.statement;
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Round ${view.round} · pair ${g.number}`),
        el("p", { className: "clock", id: "p-clock" }),
        el("p", { className: "note" }, "Go and find"),
        el("p", { className: "big" }, g.partners.map((p) => p.name).join(" and ")),
        el("p", { className: "statement" }, s.text),
        el(
          "div",
          { className: "facts" },
          el("div", { className: "fact" }, el("span", {}, "You"), el("b", {}, said(s, g.before))),
          ...g.partners.map((p) =>
            el("div", { className: "fact" }, el("span", {}, p.name), el("b", {}, said(s, p.answer))),
          ),
        ),
        el(
          "p",
          { className: "note" },
          "Take turns saying why you think what you think. Look for what would change your mind, not for how to win.",
        ),
      );
      return;
    }
    if (view.phase === "updating" && g) {
      const s = g.statement;
      const value = el("span", { className: "value" }, said(s, g.after ?? g.before));
      const input = slider(s, (v) => run(answer(s.id, v)), value);
      input.value = String(g.after ?? g.before);
      input.classList.remove("unset");
      const same = el("button", { type: "button", className: "button" }, "Where I was");
      same.addEventListener("click", () => run(answer(s.id, g.before)));
      const partners = g.partners
        .filter((p) => p.after !== null)
        .map((p) => `${p.name}: ${said(s, p.answer)} → ${said(s, p.after)}`);
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Round ${view.round} is over`),
        el("h2", {}, "How much did that change your mind?"),
        el("p", { className: "statement" }, s.text),
        el("p", { className: "note" }, `Before: ${said(s, g.before)}.`),
        input,
        ends(s),
        el("div", { className: "row-foot" }, value),
        el("div", { className: "actions" }, same),
        g.after !== null
          ? el(
              "p",
              { className: g.after !== g.before ? "good" : "note" },
              g.after !== g.before ? "Updated. Thank you." : "Kept. That is an answer too.",
            )
          : el(
              "p",
              { className: "note" },
              "Move the slider to where you stand now, or keep where you were. Coming towards your partner scores for them, not you, so be honest.",
            ),
        partners.length ? el("p", { className: "note" }, partners.join(" · ")) : "",
      );
      return;
    }
    if (view.waiting) {
      box.replaceChildren(
        el("p", { className: "eyebrow" }, `Round ${view.round}`),
        el("h2", {}, "You were not paired this round"),
        el(
          "p",
          { className: "note" },
          "Nobody was left, or nobody had answered the same statements as you. Answer some more, or go and find somebody below.",
        ),
      );
      return;
    }
    if (view.phase === "finished") {
      box.replaceChildren(
        el("p", { className: "eyebrow" }, "That was the evening"),
        el("h2", {}, view.updated ? `You updated on ${plural(view.updated, "statement", "statements")}` : "You held your ground"),
        el(
          "p",
          { className: "note" },
          `Your answers moved ${view.moved} points in total since you first gave them. You moved the people you talked to ${view.persuaded.points} points${view.rank ? `, which is place ${view.rank} in the room` : ""}.`,
        ),
        view.winners.length
          ? el(
              "div",
              {},
              el("h3", {}, "Who changed the most minds"),
              el(
                "ol",
                { className: "tallies leaders" },
                ...view.winners.map((l, i) =>
                  el(
                    "li",
                    { className: i === 0 ? "first" : "" },
                    el("span", {}, el("b", {}, l.name), i === 0 ? " 🏆" : ""),
                    el("small", {}, `${l.points} points, ${plural(l.people, "person", "people")} moved`),
                  ),
                ),
              ),
            )
          : "",
      );
      return;
    }
    box.replaceChildren(
      el("p", { className: "eyebrow" }, view.round ? `Between rounds` : "Answering"),
      el("h2", {}, view.round ? "Waiting for the next round" : "Say where you stand"),
      view.round
        ? el(
            "p",
            { className: "note" },
            view.persuaded.points
              ? `You have moved the people you talked to ${view.persuaded.points} points so far${view.rank ? `: place ${view.rank} in the room` : ""}.`
              : "Nobody has moved towards you yet. The next round is another chance.",
          )
        : "",
      el(
        "p",
        { className: "note" },
        view.mode === "bingo"
          ? "Answer the statements on your card. When a round starts you are paired with whoever disagrees with you most."
          : "Answer the statements you have a view on. When a round starts you are paired with whoever disagrees with you most.",
      ),
    );
  };

  const drawOpposites = (view: PlayerView): void => {
    const box = byId("opposites");
    hide("opposites", view.opposites.length === 0);
    box.replaceChildren(
      el("h2", {}, "Who disagrees with you most"),
      el("p", { className: "note" }, "You do not have to wait for a round. Go and find one of them."),
      el(
        "ol",
        { className: "tallies" },
        ...view.opposites.map((o) =>
          el(
            "li",
            {},
            el("span", {}, el("b", {}, o.name), ` on “${o.statement.text}”`),
            el("small", {}, `You: ${said(o.statement, o.mine)} · ${o.name}: ${said(o.statement, o.theirs)}`),
          ),
        ),
      ),
    );
  };

  const draw = (view: PlayerView): void => {
    current = view;
    text("you", `You are ${view.name} · ${plural(view.players, "person", "people")} in the room`);
    text("where", view.round ? `Round ${view.round}` : "");
    const shape = `${view.mode}:${view.squares.map((s) => s.statement.id).join(",")}`;
    if (shape !== builtFor) {
      builtFor = shape;
      build(view);
    }
    const finished = view.phase === "finished";
    for (const sq of view.squares) {
      const row = rows.get(sq.statement.id);
      if (!row) continue;
      row.slider.disabled = finished;
      if (row.slider !== holding) {
        row.slider.value = String(sq.answer ?? (sq.statement.chance ? 50 : 0));
        row.slider.classList.toggle("unset", sq.answer === null);
        row.value.textContent = said(sq.statement, sq.answer);
      }
      if (row.mark) row.mark.setAttribute("aria-pressed", String(sq.marked));
      row.box.classList.toggle("answered", sq.answer !== null);
    }
    if (view.mode === "open") {
      const counts = new Map<string, [number, number]>();
      for (const sq of view.squares) {
        const c = counts.get(sq.statement.topic) ?? [0, 0];
        counts.set(sq.statement.topic, [c[0] + (sq.answer !== null ? 1 : 0), c[1] + 1]);
      }
      for (const [topic, [done, all]] of counts) {
        const n = document.getElementById(`count-${topic}`);
        if (n) n.textContent = `${done} of ${all}`;
      }
    }
    // The panel at the top is rebuilt only when what it shows changes,
    // so a slider in it is not reset under a finger.
    const nowKey = JSON.stringify([
      view.phase,
      view.round,
      view.group,
      view.waiting,
      view.updated,
      view.moved,
      view.mode,
      view.persuaded,
      view.rank,
      view.winners,
    ]);
    if (nowKey !== drawnNow || !byId("now").contains(holding)) {
      if (nowKey !== drawnNow) {
        drawnNow = nowKey;
        drawNow(view);
      }
    }
    drawOpposites(view);
    drawGrid(view);
  };
  let drawnNow = "";

  suggestions("suggest-form", "idea", "idea-chance", "idea-done");

  const feed = watch<PlayerView>(at(""), draw);
  ticking(
    feed.now,
    () => (current && current.phase === "talking" ? current.ends : null),
    () => {
      const c = document.getElementById("p-clock");
      return c ? [c] : [];
    },
  );
}

const where = route();
if (where.kind === "host") host(where.key);
else if (where.kind === "join") join(where.key);
else if (where.kind === "player") player(where.key);
else start();
