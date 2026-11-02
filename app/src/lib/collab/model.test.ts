import { describe, expect, it } from "vitest";
import * as Y from "yjs";
import {
  actOrder,
  addScene,
  applyText,
  baseFromDisk,
  deleteActInDoc,
  deleteEpisodeInDoc,
  foldExternal,
  mergeText,
  orderAt,
  pickMeta,
  planCleanup,
  planContent,
  planOrder,
  planStructure,
  readDoc,
  sceneBody,
  scenesMap,
  seedFromDisk,
  setMeta,
  updateBase,
  type Base,
  type DiskScene,
  type DiskSnapshot,
  type Op,
} from "./model";

// ---------------------------------------------------------------- an in-memory "folder"

class FakeDisk {
  /** Act folders as [episode, act]; episode null outside a series. */
  acts: [number | null, number][] = [];
  scenes: (DiskScene & { order: number })[] = [];
  files: Record<string, string> = {};
  private v = 0;

  snap(): DiskSnapshot {
    const acts = [...this.acts]
      .sort((a, b) => (a[0] ?? 0) - (b[0] ?? 0) || a[1] - b[1])
      .map(([e, n]) => ({
      number: n,
      episode: e,
      title: `Act ${n}`,
      scenes: this.scenes
        .filter((s) => s.act === n && (s.episode ?? null) === e)
        .sort((a, b) => a.order - b.order)
        .map((s) => ({ id: s.id }) as never),
    }));
    return {
      view: { title: "Play", acts } as never,
      scenes: this.scenes.map((s) => ({ ...s, meta: { ...s.meta } })),
      files: Object.entries(this.files).map(([name, text]) => ({ name, text, version: "f" })),
    };
  }

  apply(op: Op) {
    const bump = (s: DiskScene) => (s.version = `v${++this.v}`);
    switch (op.kind) {
      case "act_ensure":
        if (!this.acts.some(([e, a]) => e === op.episode && a === op.act)) this.acts.push([op.episode, op.act]);
        break;
      case "act_remove":
        if (this.scenes.some((s) => s.act === op.act && (s.episode ?? null) === op.episode)) throw new Error("not empty");
        this.acts = this.acts.filter(([e, a]) => !(e === op.episode && a === op.act));
        break;
      case "create": {
        const here = this.scenes.filter((s) => s.act === op.act && (s.episode ?? null) === op.episode);
        const order = Math.max(-1, ...here.map((s) => s.order)) + 1;
        this.scenes.push({ id: op.id, act: op.act, episode: op.episode, order, body: "", version: "v0", meta: { title: op.title } as never });
        break;
      }
      case "trash":
        this.scenes = this.scenes.filter((s) => s.id !== op.id);
        break;
      case "reorder":
        op.ids.forEach((id, i) => {
          const s = this.scenes.find((x) => x.id === id)!;
          s.act = op.act;
          s.episode = op.episode;
          s.order = i;
          bump(s);
        });
        break;
      case "meta": {
        const s = this.scenes.find((x) => x.id === op.id)!;
        Object.assign(s.meta, op.patch);
        bump(s);
        break;
      }
      case "body": {
        const s = this.scenes.find((x) => x.id === op.id)!;
        s.body = op.body;
        bump(s);
        break;
      }
      case "file":
        this.files[op.name] = op.text;
    }
  }
}

/** One full reconcile pass, as the session runs it. */
function reconcile(doc: Y.Doc, disk: FakeDisk, base: Base): Base {
  foldExternal(doc, disk.snap(), base);
  const want = readDoc(doc);
  for (const op of planStructure(disk.snap(), want)) disk.apply(op);
  for (const op of planOrder(disk.snap(), want)) disk.apply(op);
  for (const op of planContent(disk.snap(), want)) disk.apply(op);
  for (const op of planCleanup(disk.snap(), want)) disk.apply(op);
  return updateBase(base, disk.snap(), want);
}

function sync(a: Y.Doc, b: Y.Doc) {
  Y.applyUpdate(b, Y.encodeStateAsUpdate(a, Y.encodeStateVector(b)));
  Y.applyUpdate(a, Y.encodeStateAsUpdate(b, Y.encodeStateVector(a)));
}

function diskState(d: FakeDisk) {
  const s = d.snap();
  return {
    acts: s.view.acts.map((a) => ({ e: a.episode, n: a.number, ids: a.scenes.map((x) => x.id) })),
    scenes: Object.fromEntries(s.scenes.map((x) => [x.id, { body: x.body, meta: pickMeta(x.meta) }])),
    files: d.files,
  };
}

function sampleLike(): FakeDisk {
  const d = new FakeDisk();
  d.acts = [
    [null, 1],
    [null, 2],
  ];
  d.scenes = [
    { id: "a", act: 1, order: 0, body: "Rain.\n\nKING\nHello.\n", version: "1", meta: { title: "Opening", status: "draft" } as never },
    { id: "b", act: 1, order: 1, body: "Night.\n", version: "1", meta: { title: "Second", tags: ["x"] } as never },
    { id: "c", act: 2, order: 0, body: "Dawn.\n", version: "1", meta: { title: "Third" } as never },
  ];
  d.files = { "project.yaml": "title: Play\n", "characters.yaml": "[]\n" };
  return d;
}

// ---------------------------------------------------------------- tests

describe("text helpers", () => {
  it("applyText makes minimal edits", () => {
    const doc = new Y.Doc();
    const t = doc.getText("t");
    t.insert(0, "The lighthouse is empty.");
    let deletes = 0;
    t.observe((e) => e.delta.forEach((d) => d.delete && (deletes += d.delete)));
    applyText(t, "The lighthouse is quite empty.");
    expect(t.toString()).toBe("The lighthouse is quite empty.");
    expect(deletes).toBe(0);
  });
  it("mergeText keeps both sides' non-overlapping edits", () => {
    const base = "Line one.\nLine two.\nLine three.\n";
    const theirs = "Line one!\nLine two.\nLine three.\n";
    const ours = "Line one.\nLine two.\nLine three, edited.\n";
    expect(mergeText(base, theirs, ours)).toBe("Line one!\nLine two.\nLine three, edited.\n");
  });
});

describe("seeding and joining", () => {
  it("a joiner's empty folder ends up identical to the sharer's", () => {
    const diskA = sampleLike();
    const docA = new Y.Doc();
    seedFromDisk(docA, diskA.snap());
    const baseA = baseFromDisk(diskA.snap());

    const docB = new Y.Doc();
    sync(docA, docB);
    const diskB = new FakeDisk();
    diskB.files = { ...readDoc(docB).files }; // project_join writes the files
    reconcile(docB, diskB, { scenes: {}, files: { ...diskB.files }, acts: [] });

    expect(diskState(diskB)).toEqual(diskState(diskA));
    // and the sharer has nothing to do
    expect(reconcile(docA, diskA, baseA)).toBeDefined();
    expect(diskState(diskA)).toEqual(diskState(diskB));
  });
});

describe("two writers", () => {
  function pair() {
    const diskA = sampleLike();
    const docA = new Y.Doc();
    seedFromDisk(docA, diskA.snap());
    let baseA = baseFromDisk(diskA.snap());
    const docB = new Y.Doc();
    sync(docA, docB);
    const diskB = new FakeDisk();
    diskB.files = { ...readDoc(docB).files };
    let baseB = reconcile(docB, diskB, { scenes: {}, files: { ...diskB.files }, acts: [] });
    const step = () => {
      sync(docA, docB);
      baseA = reconcile(docA, diskA, baseA);
      baseB = reconcile(docB, diskB, baseB);
      sync(docA, docB);
      baseA = reconcile(docA, diskA, baseA);
      baseB = reconcile(docB, diskB, baseB);
    };
    return { docA, docB, diskA, diskB, step };
  }

  it("concurrent typing in the same scene merges", () => {
    const { docA, docB, diskA, diskB, step } = pair();
    sceneBody(docA, "a")!.insert(0, "Thunder. ");
    const tb = sceneBody(docB, "a")!;
    tb.insert(tb.length, "\nROSALIND\nHi.\n");
    step();
    expect(diskA.scenes.find((s) => s.id === "a")!.body).toBe("Thunder. Rain.\n\nKING\nHello.\n\nROSALIND\nHi.\n");
    expect(diskState(diskA)).toEqual(diskState(diskB));
  });

  it("new scenes, moves, renames and trash reach the other writer", () => {
    const { docA, docB, diskA, diskB, step } = pair();
    const stA = readDoc(docA);
    // A: new scene between a and b
    addScene(docA, "n", 1, orderAt(stA, actOrder(stA, 1), 1), pickMeta({ title: "Interlude" }), "Silence.\n");
    // B: move c to the top of act 1, rename b, trash nothing yet
    const stB = readDoc(docB);
    const c = scenesMap(docB).get("c")!;
    docB.transact(() => {
      c.set("act", 1);
      c.set("order", orderAt(stB, actOrder(stB, 1), 0));
    });
    setMeta(docB, "b", { title: "The second scene", status: "done" });
    step();
    // A trashes a
    scenesMap(docA).get("a")!.set("trashed", true);
    step();
    expect(diskState(diskA)).toEqual(diskState(diskB));
    const s = diskState(diskA);
    expect(s.acts.find((x) => x.n === 1)!.ids).toEqual(["c", "n", "b"]);
    expect(s.scenes.b.meta.title).toBe("The second scene");
    expect(s.scenes.a).toBeUndefined();
  });

  it("an edit made outside the app is merged, not overwritten", () => {
    const { docA, docB, diskA, diskB, step } = pair();
    // B types in the shared doc while A's file is edited in another program
    const tb = sceneBody(docB, "b")!;
    tb.insert(tb.length, "\nAn owl calls.\n");
    diskA.scenes.find((s) => s.id === "b")!.body = "Night falls.\n";
    step();
    expect(diskA.scenes.find((s) => s.id === "b")!.body).toBe("Night falls.\n\nAn owl calls.\n");
    expect(diskState(diskA)).toEqual(diskState(diskB));
    expect(sceneBody(docA, "b")!.toString()).toBe("Night falls.\n\nAn owl calls.\n");
  });

  it("outside edits to metadata and project files are shared", () => {
    const { diskA, diskB, step } = pair();
    Object.assign(diskA.scenes.find((s) => s.id === "c")!.meta, { status: "done" });
    diskA.files["characters.yaml"] = "- {id: k, cue: KING}\n";
    step();
    expect(diskB.scenes.find((s) => s.id === "c")!.meta.status).toBe("done");
    expect(diskB.files["characters.yaml"]).toBe("- {id: k, cue: KING}\n");
  });

  it("a scene file deleted outside the app is trashed for everyone", () => {
    const { diskA, diskB, step } = pair();
    diskA.scenes = diskA.scenes.filter((s) => s.id !== "c");
    step();
    expect(diskB.scenes.some((s) => s.id === "c")).toBe(false);
  });

  it("offline edits on both sides converge when they reconnect", () => {
    const { docA, docB, diskA, diskB, step } = pair();
    // no sync in between: both offline
    sceneBody(docA, "a")!.insert(0, "[A offline] ");
    setMeta(docB, "a", { status: "first-edit" });
    const tb = sceneBody(docB, "a")!;
    tb.insert(tb.length, "[B offline]\n");
    step();
    const a = diskA.scenes.find((s) => s.id === "a")!;
    expect(a.body.startsWith("[A offline] ")).toBe(true);
    expect(a.body.endsWith("[B offline]\n")).toBe(true);
    expect(a.meta.status).toBe("first-edit");
    expect(diskState(diskA)).toEqual(diskState(diskB));
  });

  it("a settled pair plans no work", () => {
    const { docA, diskA, step } = pair();
    step();
    const want = readDoc(docA);
    expect([...planStructure(diskA.snap(), want), ...planOrder(diskA.snap(), want), ...planContent(diskA.snap(), want)]).toEqual([]);
  });
});

describe("series", () => {
  function series(): FakeDisk {
    const d = new FakeDisk();
    d.acts = [
      [1, 1],
      [2, 1],
    ];
    d.scenes = [
      { id: "e1a", act: 1, episode: 1, order: 0, body: "INT. HALL - DAY\n", version: "1", meta: { title: "One" } as never },
      { id: "e1b", act: 1, episode: 1, order: 1, body: "EXT. YARD - NIGHT\n", version: "1", meta: { title: "Two" } as never },
      { id: "e2a", act: 1, episode: 2, order: 0, body: "INT. COURT - DAY\n", version: "1", meta: { title: "Three" } as never },
    ];
    d.files = { "project.yaml": "title: S\nseries: true\n" };
    return d;
  }

  it("joins with every episode, and a move between episodes reaches the other writer", () => {
    const diskA = series();
    const docA = new Y.Doc();
    seedFromDisk(docA, diskA.snap());
    let baseA = baseFromDisk(diskA.snap());
    const docB = new Y.Doc();
    sync(docA, docB);
    const diskB = new FakeDisk();
    diskB.files = { ...readDoc(docB).files };
    let baseB = reconcile(docB, diskB, { scenes: {}, files: { ...diskB.files }, acts: [] });
    expect(diskState(diskB)).toEqual(diskState(diskA));

    // B moves e1b to the top of episode 2
    const st = readDoc(docB);
    const s = scenesMap(docB).get("e1b")!;
    docB.transact(() => {
      s.set("episode", 2);
      s.set("order", orderAt(st, actOrder(st, 1, 2), 0));
    });
    // A adds an act to episode 2 on disk (as episode_create / act_create would)
    diskA.acts.push([2, 2]);
    for (let i = 0; i < 2; i++) {
      sync(docA, docB);
      baseA = reconcile(docA, diskA, baseA);
      baseB = reconcile(docB, diskB, baseB);
    }
    expect(diskState(diskA)).toEqual(diskState(diskB));
    const acts = diskState(diskA).acts;
    expect(acts.find((a) => a.e === 2 && a.n === 1)!.ids).toEqual(["e1b", "e2a"]);
    expect(acts.some((a) => a.e === 2 && a.n === 2)).toBe(true);
  });
});

describe("deleting acts and episodes while co-writing", () => {
  function threeActs(): FakeDisk {
    const d = sampleLike();
    d.acts.push([null, 3]);
    d.scenes.push({ id: "d", act: 3, order: 0, body: "Dusk.\n", version: "1", meta: { title: "Fourth" } as never });
    return d;
  }

  function pairOf(make: () => FakeDisk) {
    const diskA = make();
    const docA = new Y.Doc();
    seedFromDisk(docA, diskA.snap());
    let baseA = baseFromDisk(diskA.snap());
    const docB = new Y.Doc();
    sync(docA, docB);
    const diskB = new FakeDisk();
    diskB.files = { ...readDoc(docB).files };
    let baseB = reconcile(docB, diskB, { scenes: {}, files: { ...diskB.files }, acts: [] });
    const step = () => {
      for (let i = 0; i < 3; i++) {
        sync(docA, docB);
        baseA = reconcile(docA, diskA, baseA);
        baseB = reconcile(docB, diskB, baseB);
      }
    };
    return { docA, docB, diskA, diskB, step };
  }

  it("an act deleted by one writer is gone from both folders, and later acts move up", () => {
    const { docA, diskA, diskB, step } = pairOf(threeActs);
    docA.transact(() => deleteActInDoc(docA, 2, null));
    step();
    for (const d of [diskA, diskB]) {
      const s = diskState(d);
      expect(s.acts).toEqual([
        { e: null, n: 1, ids: ["a", "b"] },
        { e: null, n: 2, ids: ["d"] },
      ]);
      expect(s.scenes.c).toBeUndefined();
    }
    // It stays deleted: the other writer's folder doesn't bring it back.
    step();
    expect(diskState(diskB).acts.map((a) => a.n)).toEqual([1, 2]);
    expect(readDoc(docA).acts.map((a) => a.act)).toEqual([1, 2]);
  });

  it("an act the other writer adds a scene to while offline survives with it", () => {
    const { docA, docB, diskA, diskB, step } = pairOf(threeActs);
    docA.transact(() => deleteActInDoc(docA, 3, null));
    // B, not yet synced, writes a new scene into act 3 on disk.
    diskB.scenes.push({ id: "z", act: 3, order: 1, body: "Late.\n", version: "1", meta: { title: "Late" } as never });
    step();
    expect(diskState(diskA)).toEqual(diskState(diskB));
    expect(diskState(diskA).acts.find((a) => a.n === 3)?.ids).toEqual(["z"]);
    expect(readDoc(docB).scenes.find((s) => s.id === "d")!.trashed).toBe(true);
  });

  it("an episode deleted by one writer renumbers the rest in both folders", () => {
    const series = () => {
      const d = new FakeDisk();
      d.acts = [
        [1, 1],
        [2, 1],
        [2, 2],
        [3, 1],
      ];
      d.scenes = [
        { id: "e1", act: 1, episode: 1, order: 0, body: "One.\n", version: "1", meta: { title: "One" } as never },
        { id: "e2", act: 1, episode: 2, order: 0, body: "Two.\n", version: "1", meta: { title: "Two" } as never },
        { id: "e2b", act: 2, episode: 2, order: 0, body: "Two B.\n", version: "1", meta: { title: "Two B" } as never },
        { id: "e3", act: 1, episode: 3, order: 0, body: "Three.\n", version: "1", meta: { title: "Three" } as never },
      ];
      d.files = { "project.yaml": "title: S\nseries: true\n" };
      return d;
    };
    const { docB, diskA, diskB, step } = pairOf(series);
    docB.transact(() => deleteEpisodeInDoc(docB, 1));
    step();
    for (const d of [diskA, diskB]) {
      expect(diskState(d).acts).toEqual([
        { e: 1, n: 1, ids: ["e2"] },
        { e: 1, n: 2, ids: ["e2b"] },
        { e: 2, n: 1, ids: ["e3"] },
      ]);
    }
  });
});
