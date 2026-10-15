import { describe, expect, it } from "vitest";
import { AUTO_CHECK_KEY, loadAutoCheck, saveAutoCheck, percent } from "./updater";

const mem = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};

describe("réglage de mise à jour automatique", () => {
  it("est actif par défaut", () => expect(loadAutoCheck(mem())).toBe(true));
  it("persiste la désactivation puis la réactivation", () => {
    const s = mem();
    saveAutoCheck(false, s);
    expect(s.getItem(AUTO_CHECK_KEY)).toBe("0");
    expect(loadAutoCheck(s)).toBe(false);
    saveAutoCheck(true, s);
    expect(loadAutoCheck(s)).toBe(true);
  });
  it("reste actif si le stockage lève une erreur", () => {
    const cassé = { getItem: () => { throw new Error("x"); }, setItem: () => { throw new Error("x"); } };
    expect(loadAutoCheck(cassé)).toBe(true);
    expect(() => saveAutoCheck(false, cassé)).not.toThrow();
  });
});

describe("progression", () => {
  it("calcule le pourcentage", () => expect(percent(50, 200)).toBe(25));
  it("plafonne à 100", () => expect(percent(300, 200)).toBe(100));
  it("renvoie null sans taille", () => expect(percent(10, undefined)).toBeNull());
});
