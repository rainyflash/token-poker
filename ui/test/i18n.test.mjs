import assert from "node:assert/strict";
import test from "node:test";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguage,
  persistLanguage,
  readStoredLanguage,
  resolveInitialLanguage,
  resolveSystemLanguage,
} from "../src/core/i18n/language-preference.ts";
import { translate } from "../src/core/i18n/messages.ts";

test("系统首选语言为中文时默认使用中文", () => {
  assert.equal(resolveSystemLanguage(["zh-Hans-CN", "en-US"]), "zh-CN");
  assert.equal(resolveSystemLanguage(["zh-TW"]), "zh-CN");
});

test("无法识别或不是中文时默认使用英文", () => {
  assert.equal(resolveSystemLanguage([]), "en");
  assert.equal(resolveSystemLanguage(undefined), "en");
  assert.equal(resolveSystemLanguage(["fr-FR", "zh-CN"]), "en");
});

test("用户手动选择优先于系统语言且只接受支持值", () => {
  assert.equal(resolveInitialLanguage("en", ["zh-CN"]), "en");
  assert.equal(resolveInitialLanguage("zh-CN", ["en-US"]), "zh-CN");
  assert.equal(resolveInitialLanguage("ja", ["zh-CN"]), "zh-CN");
  assert.equal(isLanguage("zh-CN"), true);
  assert.equal(isLanguage("zh-TW"), false);
});

test("手动切换会持久化并在下次启动恢复", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };

  persistLanguage(storage, "zh-CN");
  assert.equal(values.get(LANGUAGE_STORAGE_KEY), "zh-CN");
  assert.equal(readStoredLanguage(storage), "zh-CN");
  assert.equal(resolveInitialLanguage(readStoredLanguage(storage), ["en-US"]), "zh-CN");
});

test("中英文词典使用同一翻译键并支持变量插值", () => {
  assert.equal(translate("en", "match.seatSummary", { seated: 2, waiting: 1 }), "2 seated · 1 waiting");
  assert.equal(translate("zh-CN", "match.seatSummary", { seated: 2, waiting: 1 }), "2 位入座 · 1 位候补");
});
