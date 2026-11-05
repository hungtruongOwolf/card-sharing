// Architecture boundary rules for the TS workspace (context/wiki/engineering/change-management.md).
// Run: pnpm lint:boundaries
/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [
    {
      name: 'no-app-to-app',
      severity: 'error',
      comment: 'Apps must not import each other; share code through packages/*.',
      from: { path: '^apps/([^/]+)/' },
      to: { path: '^apps/([^/]+)/', pathNot: '^apps/$1/' },
    },
    {
      name: 'no-package-to-app',
      severity: 'error',
      comment: 'Packages are reusable building blocks and must never depend on an app.',
      from: { path: '^packages/' },
      to: { path: '^apps/' },
    },
    {
      name: 'no-circular',
      severity: 'error',
      comment: 'Circular dependencies make modules impossible to change in isolation.',
      from: {},
      to: { circular: true },
    },
    {
      name: 'not-to-unresolvable',
      severity: 'error',
      comment: 'Imports must resolve (catches typos and missing tsconfig path mappings).',
      from: {},
      to: { couldNotResolve: true },
    },
  ],
  options: {
    doNotFollow: { path: 'node_modules' },
    exclude: { path: '(^|/)(node_modules|\\.next|\\.expo|\\.turbo|dist|coverage)/' },
    tsPreCompilationDeps: true,
    // Internal packages are resolved through tsconfig paths (see pnpm-workspace.yaml).
    tsConfig: { fileName: 'tsconfig.base.json' },
    combinedDependencies: true,
    enhancedResolveOptions: {
      exportsFields: ['exports'],
      conditionNames: ['import', 'require', 'node', 'default', 'types', 'react-native'],
      extensions: ['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.json', '.d.ts'],
      mainFields: ['module', 'main', 'types', 'typings'],
    },
  },
};
