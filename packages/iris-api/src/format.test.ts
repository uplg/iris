import { describe, expect, it } from "vitest";
import { clock, duration, episodeCode, formatSize, languageLabel, percent, plural, timeLeft } from "./format";

describe("format", () => {
  it("clocks positions, unknown as dashes", () => {
    expect(clock(1930)).toBe("32:10");
    expect(clock(3723)).toBe("1:02:03");
    expect(clock(Number.NaN)).toBe("--:--");
    expect(clock(undefined)).toBe("--:--");
  });

  it("says lengths and what is left as people do", () => {
    expect(duration(45)).toBe("45 s");
    expect(duration(55 * 60)).toBe("55 min");
    expect(duration(72 * 60)).toBe("1 h 12 min");
    expect(duration(2 * 3600)).toBe("2 h");
    expect(timeLeft(23 * 60)).toBe("23 min left");
  });

  it("counts with the right noun", () => {
    expect(plural(1, "download")).toBe("1 download");
    expect(plural(3, "download")).toBe("3 downloads");
    expect(plural(2, "series", "series")).toBe("2 series");
  });

  it("writes episode codes the way the cards do", () => {
    expect(episodeCode(2, 4)).toBe("S2:E4");
    expect(episodeCode(2, null)).toBe("Season 2");
    expect(episodeCode(2, 0)).toBe("Season 2");
    expect(episodeCode(null, 19)).toBe("E19");
    expect(episodeCode(null, null)).toBeNull();
  });

  it("names language tags, jargon kept in brackets", () => {
    expect(languageLabel("fr", "short")).toBe("French (VF)");
    expect(languageLabel("vost")).toBe("Original audio, French subtitles (VOSTFR)");
    expect(languageLabel("xx")).toBeNull();
  });

  it("reads sizes and shares", () => {
    expect(formatSize(12.4 * 1024 ** 3)).toBe("12.4 GB");
    expect(percent(41.6)).toBe("42%");
  });
});
