// Strict EN purism for the web dictionary: the English locale must never
// render Russian, so no EN value may contain a single Cyrillic character.
// Mirrors the native `english_strings_are_cyrillic_free` test.
import { describe, expect, it } from 'vitest';
import { en } from '../locales/en';
import { ru } from '../locales/ru';

const hasCyrillic = (s: string) => /[\u0400-\u04FF]/.test(s);

describe('i18n purism', () => {
  it('EN dictionary contains zero Cyrillic', () => {
    const offenders = Object.entries(en).filter(([, value]) => hasCyrillic(value));
    expect(offenders).toEqual([]);
  });

  it('EN dictionary covers every RU key and vice versa', () => {
    const ruKeys = Object.keys(ru);
    const enKeys = Object.keys(en);
    expect(enKeys).toEqual(expect.arrayContaining(ruKeys));
    expect(ruKeys).toEqual(expect.arrayContaining(enKeys));
  });

  it('no EN value is empty', () => {
    const empty = Object.entries(en)
      .filter(([, value]) => value.trim() === '')
      .map(([key]) => key);
    expect(empty).toEqual([]);
  });
});
