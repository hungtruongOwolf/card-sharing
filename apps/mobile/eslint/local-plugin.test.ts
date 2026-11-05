import { RuleTester } from 'eslint';

// eslint-disable-next-line @typescript-eslint/no-require-imports
const plugin = require('./local-plugin');

const tester = new RuleTester({
  languageOptions: {
    ecmaVersion: 'latest',
    sourceType: 'module',
    parserOptions: { ecmaFeatures: { jsx: true } },
  },
});

describe('card-sharing/no-hardcoded-strings', () => {
  tester.run('no-hardcoded-strings', plugin.rules['no-hardcoded-strings'], {
    valid: [
      { code: '<Text>{t("home.hint")}</Text>' },
      { code: '<Button label={t("common.close")} variant="ghost" testID="close" />' },
      { code: '<Text>{count}</Text>' },
      { code: '<Text>{" • "}</Text>' },
      { code: '<Text>  </Text>' },
      { code: 'toast.show({ message: t("home.sent", { count }) })' },
      { code: 'const style = { title: 42 }' },
      { code: '<View accessibilityRole="button" />' },
    ],
    invalid: [
      { code: '<Text>Hello</Text>', errors: [{ messageId: 'literal' }] },
      { code: '<Text>{"Hello"}</Text>', errors: [{ messageId: 'literal' }] },
      { code: '<Text>{`Hi ${name}`}</Text>', errors: [{ messageId: 'literal' }] },
      { code: '<Text>{"Sent to " + n}</Text>', errors: [{ messageId: 'literal' }] },
      { code: '<Text>{ok ? "Yes" : t("no")}</Text>', errors: [{ messageId: 'literal' }] },
      { code: '<Button label="Close" />', errors: [{ messageId: 'literal' }] },
      { code: '<View accessibilityLabel={"Card"} />', errors: [{ messageId: 'literal' }] },
      { code: '<TextInput placeholder="Name" />', errors: [{ messageId: 'literal' }] },
      { code: 'toast.show({ message: "Saved" })', errors: [{ messageId: 'literal' }] },
      { code: '<Stack.Screen options={{ title: "Home" }} />', errors: [{ messageId: 'literal' }] },
      { code: '<Text>Ciao, мир</Text>', errors: [{ messageId: 'literal' }] },
    ],
  });
});

describe('card-sharing/rtl-safe-styles', () => {
  tester.run('rtl-safe-styles', plugin.rules['rtl-safe-styles'], {
    valid: [
      { code: 'const s = { marginStart: 4, paddingEnd: 8, start: 0, borderTopStartRadius: 4 }' },
      { code: 'const s = { textAlign: "center" }' },
      { code: 'const s = { textAlign: "auto" }' },
    ],
    invalid: [
      { code: 'const s = { marginLeft: 4 }', errors: [{ messageId: 'physical' }] },
      { code: 'const s = { paddingRight: 4 }', errors: [{ messageId: 'physical' }] },
      { code: 'const s = { left: 0 }', errors: [{ messageId: 'physical' }] },
      { code: 'const s = { borderBottomRightRadius: 2 }', errors: [{ messageId: 'physical' }] },
      { code: 'const s = { textAlign: "left" }', errors: [{ messageId: 'align' }] },
    ],
  });
});
