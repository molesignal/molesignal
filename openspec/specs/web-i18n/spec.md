# Web Application Internationalization

## Purpose

Provide a consistent i18n layer for the web application so every user-visible chrome string flows through a single translation pipeline with at least two locales (en, zh-CN), a locale persistence story, and a Settings dropdown affordance for runtime locale switching.

## Requirements

### Requirement: i18n Framework

The web app SHALL ship a `react-i18next`-based i18n layer with at least two locales: `en` (default) and `zh-CN`. All user-visible chrome strings (nav labels, button text, dialog headings, toast messages, palette placeholders, error/empty-state copy, aria-labels) SHALL go through `useTranslation().t(key)` and never be hard-coded in JSX.

#### Scenario: Default locale on first visit

- **WHEN** an unauthenticated browser opens `/login` without a stored language preference
- **AND** `navigator.language` starts with `zh`
- **THEN** the UI renders in zh-CN
- **AND** `document.documentElement.lang` is `zh-CN`

#### Scenario: Missing key falls back to en

- **WHEN** `t('some.key.only.in.en')` is rendered with active locale `zh-CN`
- **THEN** the English value is shown
- **AND** a `console.warn` is logged with the key name (dev build only)

### Requirement: Language Switcher

The Settings dropdown in the StatusStrip SHALL include a `Language` submenu listing all available locales; selecting one SHALL update `useThemeStore.language`, persist to `localStorage['molesignal-lang']`, and re-render with the new locale within one frame.

#### Scenario: User switches to zh-CN

- **WHEN** the user opens Settings → Language → 中文 (zh-CN)
- **THEN** every translated string in the visible viewport updates immediately
- **AND** `document.documentElement.lang` becomes `zh-CN`
- **AND** the next page reload still shows zh-CN

#### Scenario: Switch is keyboard accessible

- **WHEN** the user opens the Settings dropdown via keyboard (`Tab` to the gear, `Enter`)
- **AND** navigates the Language submenu via arrow keys
- **THEN** focus stays inside the dropdown and `Enter` activates the selected language
