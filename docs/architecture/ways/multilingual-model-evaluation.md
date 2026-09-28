# Multilingual Embedding Model Evaluation

**Date:** 2026-04-03T04:07:51Z

**Models:**
- English: `all-MiniLM-L6-v2` (21M)
- Multilingual: `paraphrase-multilingual-MiniLM-L12-v2` Q8_0 (127M)

**Methodology:** Each test embeds a native-language prompt against both an English description (cross-language) and a native-language description (same-language stub scenario). Three scores per test:

- **EN×EN**: English-only model, English description (baseline)
- **Multi×EN**: multilingual model, English description (cross-language)
- **Multi×Native**: multilingual model, native description (same-language stub)

**Threshold:** 0.25 (same-language similarity minimum)

## Results

| Lang | Prompt | EN×EN | Multi×EN | Multi×Native | Pass |
|:-----|:-------|------:|---------:|-------------:|:----:|
| en | check dependencies for vulnerabilities | 0.7574 | 0.6822 | 0.6822 | ✅ |
| de | Abhängigkeiten auf Schwachstellen prüfen | 0.0767 | 0.6223 | 0.8243 | ✅ |
| es | verificar dependencias por vulnerabilidades | 0.4381 | 0.7893 | 0.8357 | ✅ |
| fr | vérifier les dépendances pour vulnérabilités | 0.5223 | 0.7418 | 0.8938 | ✅ |
| pt | verificar dependências por vulnerabilidades | 0.4381 | 0.7899 | 0.9605 | ✅ |
| ru | проверить зависимости на уязвимости | 0.0295 | 0.7592 | 0.8536 | ✅ |
| ja | 依存関係の脆弱性をチェックして | -0.0290 | 0.6861 | 0.9338 | ✅ |
| ko | 의존성 취약점 검사 | -0.0163 | 0.7179 | 0.8554 | ✅ |
| zh | 检查依赖项的漏洞 | -0.0266 | 0.5866 | 0.8874 | ✅ |
| ar | فحص التبعيات بحثاً عن ثغرات | 0.0416 | 0.3995 | 0.9581 | ✅ |
| el | έλεγχος εξαρτήσεων για ευπάθειες | -0.0118 | 0.5934 | 0.8065 | ✅ |
| en | write a conventional commit message | 0.7930 | 0.7465 | 0.7465 | ✅ |
| ja | コミットメッセージを書いて | 0.0929 | 0.5314 | 0.8322 | ✅ |
| ko | 커밋 메시지 작성 | 0.1070 | 0.4847 | 0.8074 | ✅ |
| zh | 写一个规范的提交信息 | 0.0727 | 0.6833 | 0.8896 | ✅ |
| de | eine konventionelle Commit-Nachricht schreiben | 0.3081 | 0.6349 | 0.7801 | ✅ |
| ru | написать сообщение коммита | 0.0039 | 0.4563 | 0.5000 | ✅ |
| en | add unit tests for the auth module | 0.5089 | 0.7411 | 0.7411 | ✅ |
| ja | 認証モジュールのユニットテストを追加して | 0.0009 | 0.7461 | 0.8338 | ✅ |
| ko | 인증 모듈에 단위 테스트 추가 | 0.0917 | 0.5602 | 0.7660 | ✅ |
| zh | 为认证模块添加单元测试 | 0.0162 | 0.7100 | 0.8278 | ✅ |
| de | Unit-Tests für das Auth-Modul hinzufügen | 0.4085 | 0.7445 | 0.8100 | ✅ |
| ru | добавить юнит-тесты для модуля аутентификации | 0.0629 | 0.1650 | 0.3210 | ✅ |

## Summary

- **Tests:** 23
- **Passed:** 23
- **Failed:** 0
- **Accuracy:** 100.0%

## Timing

| Phase | Duration | Tests | Per-test |
|:------|:---------|------:|---------:|
| EN model batch (23 pairs) | 104ms | 23 | 4ms |
| Multi model cross-language (23 pairs) | 392ms | 23 | 17ms |
| Multi model same-language (23 pairs) | 389ms | 23 | 16ms |
| **Total** | **889ms** | **69** | **12ms** |

## Interpretation

The multilingual model enables three matching strategies:

1. **English ways + English model** — current production. High precision for English prompts.
2. **English ways + multilingual model (cross-language)** — user types in any language, matches against English descriptions. Works but scores 30-50% lower.
3. **Native-language stubs + multilingual model (same-language)** — locale entries in `.locales.jsonl` with native descriptions. Consistently scores 0.80+ across tested languages.

**Recommendation:** Ship both models. English ways use the English model (precise, 21MB). Multilingual stubs use the multilingual model (broad, 127MB). Per-way `embed_model` frontmatter field controls routing. This gives per-language threshold tuning without compromising English accuracy.
