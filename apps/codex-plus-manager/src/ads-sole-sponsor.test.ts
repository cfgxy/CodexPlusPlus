import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { readFile } from "node:fs/promises";

// 注入脚本是与 Rust 后端并列的第二条取数路径，它自己 normalize、不经 Rust。
// 这里按仓库既有做法切出真实源码片段并用 new Function 实际执行，
// 而不是只断言源码里存在某个字符串——字符串存在证明不了分类行为正确。

type Ad = {
  id: string;
  type: string;
  title: string;
  description: string;
  url: string;
  image?: string;
  expires_at?: string;
  highlights?: string[];
};

async function readRenderer() {
  return readFile(new URL("../../../assets/inject/renderer-inject.js", import.meta.url), "utf8");
}

function sliceBetween(renderer: string, startMarker: string, endMarker: string) {
  const start = renderer.indexOf(startMarker);
  const end = renderer.indexOf(endMarker, start);
  assert.ok(start >= 0, `未找到片段起点：${startMarker}`);
  assert.ok(end > start, `未找到片段终点：${endMarker}`);
  return renderer.slice(start, end);
}

type FetchStub = {
  postJson: (url: string, body: unknown) => Promise<unknown>;
  fetch: (url: string, init?: unknown) => Promise<{ ok: boolean; status: number; json: () => Promise<unknown> }>;
};

type AdsRuntime = {
  enforceCodexPlusSoleSponsor: (ads: Ad[]) => Ad[];
  normalizeCodexPlusAds: (payload: unknown) => Ad[];
  fetchCodexPlusAds: () => Promise<void>;
  readAds: () => Ad[];
  diagnostics: Array<{ event: string; detail: unknown }>;
  fetchedUrls: string[];
};

// 赞助位 banner 由 Rust 侧 injection_script_with_settings 写进这个 window 变量，
// 值取自 docs/images/sponsor-apinoria.png 的同一份 include_bytes! 资产。
// 测试直接读这张图算出 data URI，最终 image 字段必须与它逐字节一致。
async function sponsorBannerDataUrl() {
  const bytes = await readFile(
    new URL("../../../docs/images/sponsor-apinoria.png", import.meta.url),
  );
  return `data:image/png;base64,${bytes.toString("base64")}`;
}

function adsRuntime(renderer: string, stub: FetchStub, banner = ""): AdsRuntime {
  // 两段真实源码：分类段（常量 → normalize 结束）与取数段（缓存穿透 → fetch 结束）。
  const classification = sliceBetween(
    renderer,
    '  const codexPlusAdsUrl = "/ads";',
    "  function renderCodexPlusAdGroup(",
  );
  const fetching = sliceBetween(
    renderer,
    "  function cacheBustCodexPlusAdUrl(url, version) {",
    "  function selectCodexPlusTab(tab) {",
  );
  const diagnostics: Array<{ event: string; detail: unknown }> = [];
  const fetchedUrls: string[] = [];
  // renderCodexPlusAds 只在拿到面板节点时才被求值；桩 document 返回 null，
  // 因此不需要把渲染段一起切进来。
  const document = { querySelector: () => null };
  const windowStub = { __CODEX_PLUS_APINORIA_BANNER__: banner };
  const create = new Function(
    "postJson",
    "fetch",
    "document",
    "sendCodexPlusDiagnostic",
    "recordFetchedUrl",
    "window",
    `${classification}\n${fetching}\nreturn {
      enforceCodexPlusSoleSponsor,
      normalizeCodexPlusAds,
      fetchCodexPlusAds,
      readAds: () => codexPlusAds,
    };`,
  ) as (
    postJson: FetchStub["postJson"],
    fetchValue: FetchStub["fetch"],
    documentValue: typeof document,
    sendCodexPlusDiagnostic: (event: string, detail: unknown) => void,
    recordFetchedUrl: (url: string) => void,
    windowValue: typeof windowStub,
  ) => Omit<AdsRuntime, "diagnostics" | "fetchedUrls">;

  const runtime = create(
    stub.postJson,
    async (url: string, init?: unknown) => {
      fetchedUrls.push(url);
      return stub.fetch(url, init);
    },
    document,
    (event, detail) => diagnostics.push({ event, detail }),
    (url: string) => fetchedUrls.push(url),
    windowStub,
  );
  return { ...runtime, diagnostics, fetchedUrls };
}

function ad(overrides: Partial<Ad> & { id: string; url: string }): Ad {
  return {
    type: "sponsor",
    title: `标题 ${overrides.id}`,
    description: `描述 ${overrides.id}`,
    ...overrides,
  };
}

const idleStub: FetchStub = {
  postJson: async () => ({ ads: [] }),
  fetch: async () => ({ ok: false, status: 500, json: async () => ({}) }),
};

describe("注入脚本的赞助商分类行为", () => {
  it("Apinoria 是唯一 sponsor 且排在首位，其他条目保留并降为 normal", async () => {
    const runtime = adsRuntime(await readRenderer(), idleStub);

    const result = runtime.enforceCodexPlusSoleSponsor([
      ad({ id: "remote-sponsor", url: "https://example.test/" }),
      ad({ id: "remote-normal", type: "normal", url: "https://example.org/" }),
    ]);

    assert.equal(result.filter((item) => item.type === "sponsor").length, 1);
    assert.equal(result[0].id, "apinoria");
    assert.equal(result[0].url, "https://www.apinoria.com/");
    // 降级不等于删除：这些条目仍要作为普通推荐展示。
    assert.deepEqual(
      result.slice(1).map((item) => [item.id, item.type]),
      [
        ["remote-sponsor", "normal"],
        ["remote-normal", "normal"],
      ],
    );
  });

  it("远端自带 Apinoria 时去重，主站与裸域都算同一家", async () => {
    const runtime = adsRuntime(await readRenderer(), idleStub);

    const result = runtime.enforceCodexPlusSoleSponsor([
      ad({ id: "apinoria", url: "https://www.apinoria.com/" }),
      ad({ id: "apinoria-www", url: "https://www.apinoria.com/pricing" }),
      ad({ id: "apinoria-apex", url: "https://apinoria.com/" }),
      ad({ id: "keep-me", url: "https://example.test/" }),
    ]);

    assert.deepEqual(
      result.map((item) => item.id),
      ["apinoria", "keep-me"],
    );
    assert.equal(result.filter((item) => item.type === "sponsor").length, 1);
  });

  it("URL 里只是含有 apinoria.com 文本的其他条目必须保留并降级", async () => {
    // 按整条 URL 做子串包含会把这三条合法推荐当成重复赞助位整条删掉。
    const runtime = adsRuntime(await readRenderer(), idleStub);

    const lookalikes = [
      ad({ id: "query-param-lookalike", url: "https://other.example/?source=apinoria.com" }),
      ad({ id: "suffix-domain-lookalike", url: "https://apinoria.com.example/" }),
      ad({ id: "prefix-domain-lookalike", url: "https://notapinoria.com/" }),
    ];
    const result = runtime.enforceCodexPlusSoleSponsor(lookalikes);

    assert.equal(result.filter((item) => item.type === "sponsor").length, 1);
    assert.deepEqual(
      result.slice(1).map((item) => [item.id, item.type]),
      lookalikes.map((item) => [item.id, "normal"]),
    );
  });

  it("本地后端返回空结果时进入 raw→jsDelivr 回退，且顺序不能颠倒", async () => {
    const runtime = adsRuntime(await readRenderer(), {
      postJson: async () => ({ ads: [] }),
      fetch: async (url: string) => {
        if (String(url).includes("raw.githubusercontent.com")) {
          return { ok: false, status: 404, json: async () => ({}) };
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            version: 1,
            ads: [ad({ id: "from-jsdelivr", url: "https://example.test/" })],
          }),
        };
      },
    });

    await runtime.fetchCodexPlusAds();

    assert.equal(runtime.fetchedUrls.length, 2);
    assert.match(runtime.fetchedUrls[0], /^https:\/\/raw\.githubusercontent\.com\/cfgxy\/CodexPlusPlus\/main\/ads\.json\?v=\d+$/);
    assert.match(runtime.fetchedUrls[1], /^https:\/\/cdn\.jsdelivr\.net\/gh\/cfgxy\/CodexPlusPlus@main\/ads\.json\?v=\d+$/);
    assert.deepEqual(
      runtime.readAds().map((item) => [item.id, item.type]),
      [
        ["apinoria", "sponsor"],
        ["from-jsdelivr", "normal"],
      ],
    );
  });

  it("双源全部失败时记录诊断，赞助位仍然存在", async () => {
    const runtime = adsRuntime(await readRenderer(), {
      postJson: async () => {
        throw new Error("本地后端不可用");
      },
      fetch: async () => ({ ok: false, status: 503, json: async () => ({}) }),
    });

    await runtime.fetchCodexPlusAds();

    assert.deepEqual(
      runtime.readAds().map((item) => [item.id, item.type]),
      [["apinoria", "sponsor"]],
    );
    assert.equal(runtime.diagnostics.length, 1);
    assert.equal(runtime.diagnostics[0].event, "ads_fetch_failed");
  });

  it("赞助位主图取自 Rust 注入的 banner，与仓库资产逐字节一致", async () => {
    const banner = await sponsorBannerDataUrl();
    const runtime = adsRuntime(await readRenderer(), idleStub, banner);

    const result = runtime.enforceCodexPlusSoleSponsor([]);

    assert.equal(result[0].id, "apinoria");
    assert.ok(result[0].image, "赞助位主图不能为空");
    assert.equal(result[0].image, banner);
  });

  it("双源全部失败的兜底赞助位也带主图", async () => {
    // 数据源抖动时赞助位仍要出现，缺图等于赞助商展示打折。
    const banner = await sponsorBannerDataUrl();
    const runtime = adsRuntime(
      await readRenderer(),
      {
        postJson: async () => {
          throw new Error("本地后端不可用");
        },
        fetch: async () => ({ ok: false, status: 503, json: async () => ({}) }),
      },
      banner,
    );

    await runtime.fetchCodexPlusAds();

    assert.equal(runtime.readAds()[0].image, banner);
  });

  it("本地后端已给出内容时不触发远端回退", async () => {
    const runtime = adsRuntime(await readRenderer(), {
      postJson: async () => ({
        ads: [ad({ id: "from-local", type: "normal", url: "https://example.test/" })],
      }),
      fetch: async () => {
        throw new Error("不应触发远端回退");
      },
    });

    await runtime.fetchCodexPlusAds();

    assert.deepEqual(runtime.fetchedUrls, []);
    assert.deepEqual(
      runtime.readAds().map((item) => item.id),
      ["apinoria", "from-local"],
    );
  });
});
