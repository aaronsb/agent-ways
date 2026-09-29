# Multilingual Way Matching Integration Test

## Instructions for Claude

Read this file with the Read tool — do NOT have the user paste it into chat.

You are running an integration test for multilingual way matching. This test verifies that locale stubs fire correctly when prompts are written in non-English languages, and that the embedding matching engine can route native-language queries to the right way.

**Your role**: Follow each step in order. Announce what step you are on, perform the action, then report the result against the expected outcome. Wait for the user to complete each USER step before moving on.

### How to verify what fired

After the user types each prompt, check **two** signals:

1. **System-reminders injected into your context** — look for any new `<system-reminder>` blocks that appeared after the user's message. These contain the actual way content that the hook pipeline delivered. Report what headings and content you see.

2. **Embedding scoring via CLI** — run this command (from `~/.claude/`) to see how the prompt scores:
   ```bash
   ways match "the exact prompt the user typed"
   ```
   This prints the ranked candidates with peak, share, confirm and outcome (`fired ✓`, `< gate`, `< confirm`). The top-scoring way should match the expected one. Report the top 3 results with their scores.

Use **both** signals: system-reminders confirm the hook pipeline delivered content; `ways match` confirms the scoring engine ranked the right way highest.

### Report format

```
Step N: [description]
Result: PASS / FAIL / UNEXPECTED
Injected: [what system-reminder content appeared, or "none"]
Embedding top 3:
  #1 way/id  score  description snippet
  #2 way/id  score  description snippet
  #3 way/id  score  description snippet
Detail: [assessment vs expected]
```

After reading this file, begin with Step 1.

---

## Part A: Latin-Script Languages

These use the same script as English but should match locale-specific vocabulary.

### Step 1 — German: Architecture decision

> **USER**: Type exactly: `Ich muss eine Architekturentscheidung dokumentieren, ein ADR erstellen`

> **CLAUDE**: Check system-reminders for ADR content, then run:
> ```bash
> cd ~/.claude && ways match "Ich muss eine Architekturentscheidung dokumentieren, ein ADR erstellen"
> ```
> Report the top 3 matches.

**Expected**: The ADR way (`architecture/adr`) should be #1. The German locale vocabulary includes: Architektur, Entscheidung, ADR, Entwurf, Muster.

---

### Step 2 — Spanish: Code testing

> **USER**: Type exactly: `necesito escribir pruebas unitarias para este modulo`

> **CLAUDE**: Check system-reminders for testing content, then run:
> ```bash
> cd ~/.claude && ways match "necesito escribir pruebas unitarias para este modulo"
> ```
> Report the top 3 matches.

**Expected**: The testing way (`code/testing`) should be #1. Spanish locale vocabulary includes: prueba, cobertura, test, verificar, unitaria.

---

### Step 3 — French: Dependency management

> **USER**: Type exactly: `il faut installer les dependances et mettre a jour les paquets`

> **CLAUDE**: Check system-reminders for deps content, then run:
> ```bash
> cd ~/.claude && ways match "il faut installer les dependances et mettre a jour les paquets"
> ```
> Report the top 3 matches.

**Expected**: The deps way (`environment/deps`) should be #1. French locale vocabulary includes: dependance, paquet, biblioteque, installer.

---

### Step 4 — Portuguese (Brazil): Pull request

> **USER**: Type exactly: `preciso criar um pull request e fazer merge na branch principal`

> **CLAUDE**: Check system-reminders for GitHub/delivery content, then run:
> ```bash
> cd ~/.claude && ways match "preciso criar um pull request e fazer merge na branch principal"
> ```
> Report the top 3 matches.

**Expected**: The GitHub way (`delivery/github`) should be in the top 3. The pt-br locale vocabulary includes: PR, pullrequest, revisao, branch, merge.

---

## Part B: CJK Languages

These use non-Latin scripts and test the embedding engine's cross-script capability.

### Step 5 — Japanese: Debugging

> **USER**: Type exactly: `このバグを調査してデバッグしたい、スタックトレースを確認する`

> **CLAUDE**: Check system-reminders for debugging content, then run:
> ```bash
> cd ~/.claude && ways match "このバグを調査してデバッグしたい、スタックトレースを確認する"
> ```
> Report the top 3 matches.

**Expected**: The debugging way (`environment/debugging`) should be in top 3. Japanese locale vocabulary includes: デバッグ, 調査, バグ, スタックトレース. CJK matching is routed through the embedding engine's alias model.

---

### Step 6 — Korean: Security review

> **USER**: Type exactly: `코드 보안 검토가 필요합니다, 취약점을 확인해야 합니다`

> **CLAUDE**: Check system-reminders for security content, then run:
> ```bash
> cd ~/.claude && ways match "코드 보안 검토가 필요합니다, 취약점을 확인해야 합니다"
> ```
> Report the top 3 matches.

**Expected**: The security way (`code/security`) should be in the top 3. Korean locale vocabulary includes: 보안, 취약점.

---

### Step 7 — Chinese: Performance optimization

> **USER**: Type exactly: `需要优化性能，分析瓶颈和延迟问题`

> **CLAUDE**: Check system-reminders for performance content, then run:
> ```bash
> cd ~/.claude && ways match "需要优化性能，分析瓶颈和延迟问题"
> ```
> Report the top 3 matches.

**Expected**: The performance way (`code/performance`) should be in top 3. Chinese locale vocabulary includes: 优化, 性能, 瓶颈, 延迟. Note: CJK matching relies on the embedding engine.

---

## Part C: Cyrillic and Other Scripts

### Step 8 — Russian: Git commit conventions

> **USER**: Type exactly: `нужно сделать коммит с правильным сообщением и запушить`

> **CLAUDE**: Check system-reminders for commit/delivery content, then run:
> ```bash
> cd ~/.claude && ways match "нужно сделать коммит с правильным сообщением и запушить"
> ```
> Report the top 3 matches.

**Expected**: The commits way (`delivery/commits`) should be #1. Russian locale vocabulary includes: коммит, сообщение, ветка, слияние.

---

### Step 9 — Ukrainian: Environment setup

> **USER**: Type exactly: `потрібно налаштувати середовище розробки та встановити залежності`

> **CLAUDE**: Check system-reminders for environment content, then run:
> ```bash
> cd ~/.claude && ways match "потрібно налаштувати середовище розробки та встановити залежності"
> ```
> Report the top 3 matches.

**Expected**: The environment way (`softwaredev/environment`) should be #1. Ukrainian locale vocabulary includes: середовище, налаштування, конфігурація, залежність, встановлення.

---

### Step 10 — Arabic: Task management

> **USER**: Type exactly: `أحتاج إلى إدارة المهام ومتابعة بنود العمل المعلقة`

> **CLAUDE**: Check system-reminders for task management content, then run:
> ```bash
> cd ~/.claude && ways match "أحتاج إلى إدارة المهام ومتابعة بنود العمل المعلقة"
> ```
> Report the top 3 matches.

**Expected**: The tasks way (`ea/tasks`) should be #1. Arabic locale vocabulary includes: مهام, إدارة, بنود, عمل, متابعة.

---

## Part D: Southeast Asian and Indic Scripts

### Step 11 — Thai: Documentation

> **USER**: Type exactly: `ต้องเขียนเอกสารประกอบโค้ดและคู่มือเริ่มต้นใช้งาน`

> **CLAUDE**: Check system-reminders for documentation content, then run:
> ```bash
> cd ~/.claude && ways match "ต้องเขียนเอกสารประกอบโค้ดและคู่มือเริ่มต้นใช้งาน"
> ```
> Report the top 3 matches.

**Expected**: A docs way (`docs/docs` or `docs/readme`) should be in the top 3. Thai locale vocabulary includes: เอกสาร, มาร์กดาวน์.

---

### Step 12 — Hindi: Supply chain security

> **USER**: Type exactly: `इस रिपॉजिटरी की सप्लाई चेन सुरक्षा ऑडिट करनी है`

> **CLAUDE**: Check system-reminders for supply chain content, then run:
> ```bash
> cd ~/.claude && ways match "इस रिपॉजिटरी की सप्लाई चेन सुरक्षा ऑडिट करनी है"
> ```
> Report the top 3 matches.

**Expected**: The supply chain way (`code/supplychain`) should be #1. Hindi locale vocabulary includes: सप्लाई चेन, ऑडिट, सुरक्षा, रिपॉजिटरी.

---

## Part E: Cross-Language Consistency

### Step 13 — Same concept, different language: English baseline

> **USER**: Type exactly: `I need to write unit tests with mocks for the database layer`

> **CLAUDE**: Run:
> ```bash
> cd ~/.claude && ways match "I need to write unit tests with mocks for the database layer"
> ```
> Report top 3 and note which testing/mocking ways appeared. This establishes a baseline for Step 14.

**Expected**: The testing way and/or mocking way should be in the top results.

---

### Step 14 — Same concept in Italian

> **USER**: Type exactly: `devo scrivere test unitari con mock per il livello database`

> **CLAUDE**: Run:
> ```bash
> cd ~/.claude && ways match "devo scrivere test unitari con mock per il livello database"
> ```
> Report top 3 and compare against Step 13. Did the same testing/mocking ways appear?

**Expected**: The same testing/mocking ways should fire as in Step 13. If different ways fire, that indicates a locale vocabulary gap.

---

## Part F: Negative Tests

### Step 15 — Non-active language should not match locale stubs

> **USER**: Type exactly: `Ik moet de code refactoren en de kwaliteit verbeteren` (Dutch)

> **CLAUDE**: Run:
> ```bash
> cd ~/.claude && ways match "Ik moet de code refactoren en de kwaliteit verbeteren"
> ```
> Check: did any ways match? If so, are they matching English vocabulary overlap (words like "code", "refactoren" ≈ "refactor") or Dutch locale entries? Dutch (nl) is inactive — its locale stubs were removed.

**Expected**: Ways may fire via English keyword overlap, but should NOT fire via Dutch locale entries. If a way's description column shows Dutch text, that's a bug.

---

## Part G: Summary

### Step 16 — Compile results

> **CLAUDE**: Compile a summary table:
>
> | Step | Language | Script | Expected Way | Result |
> |------|----------|--------|-------------|--------|
> | 1 | German (de) | Latin | architecture/adr | ? |
> | 2 | Spanish (es) | Latin | code/testing | ? |
> | 3 | French (fr) | Latin | environment/deps | ? |
> | 4 | Portuguese (pt-br) | Latin | delivery/github | ? |
> | 5 | Japanese (ja) | CJK | environment/debugging | ? |
> | 6 | Korean (ko) | CJK | code/security | ? |
> | 7 | Chinese (zh) | CJK | code/performance | ? |
> | 8 | Russian (ru) | Cyrillic | delivery/commits | ? |
> | 9 | Ukrainian (uk) | Cyrillic | environment/environment | ? |
> | 10 | Arabic (ar) | Arabic | ea/tasks | ? |
> | 11 | Thai (th) | Thai | docs/docs | ? |
> | 12 | Hindi (hi) | Devanagari | code/supplychain | ? |
> | 13 | English (en) | Latin | code/testing + mocking | ? |
> | 14 | Italian (it) | Latin | code/testing + mocking | ? |
> | 15 | Dutch (nl) | Latin | No locale match | ? |
>
> Report pass/fail count and observations about:
> - Whether system-reminders delivered way content vs just embedding scoring
> - Whether CJK and non-Latin scripts matched as reliably as Latin-script languages
> - Whether the cross-language consistency test (Steps 13-14) produced equivalent results
> - Whether inactive language (Dutch) correctly fell back to English-only matching
> - Any languages that consistently underperform — candidates for vocabulary revision
