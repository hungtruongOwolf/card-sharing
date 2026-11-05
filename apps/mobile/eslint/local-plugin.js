// Local ESLint rules for the mobile app (T-33). No dependency: plain ESLint 9 rule objects.
//
//   card-sharing/no-hardcoded-strings  user-facing text must come from the message catalog (`t()`)
//   card-sharing/rtl-safe-styles       use start/end instead of left/right so layouts mirror in RTL

const LETTER = /\p{L}/u;

/** Props and option keys whose string value is shown or read to the user. */
const USER_FACING = new Set([
  'accessibilityLabel',
  'accessibilityHint',
  'aria-label',
  'aria-description',
  'placeholder',
  'title',
  'headerTitle',
  'headerBackTitle',
  'tabBarLabel',
  'label',
  'message',
  'alt',
]);

const hasText = (value) => typeof value === 'string' && LETTER.test(value);

function literalText(node) {
  if (!node) return null;
  if (node.type === 'Literal' && hasText(node.value)) return node.value;
  if (node.type === 'TemplateLiteral' && node.quasis.some((q) => hasText(q.value.cooked))) {
    return node.quasis.map((q) => q.value.cooked).join('…');
  }
  if (node.type === 'ConditionalExpression') {
    return literalText(node.consequent) ?? literalText(node.alternate);
  }
  if (node.type === 'LogicalExpression') return literalText(node.right) ?? literalText(node.left);
  if (node.type === 'BinaryExpression' && node.operator === '+') {
    return literalText(node.left) ?? literalText(node.right);
  }
  return null;
}

function propName(node) {
  if (node.type === 'JSXAttribute') {
    return node.name.type === 'JSXIdentifier' ? node.name.name : null;
  }
  if (node.key.type === 'Identifier') return node.key.name;
  if (node.key.type === 'Literal') return String(node.key.value);
  return null;
}

const noHardcodedStrings = {
  meta: {
    type: 'problem',
    docs: {
      description: 'Forbid hard-coded user-facing strings; use t() from src/i18n with the catalog.',
    },
    messages: {
      literal:
        'Hard-coded user-facing string "{{text}}". Add it to src/i18n/messages/en.ts and use t().',
    },
    schema: [],
  },
  create(context) {
    const report = (node, text) =>
      context.report({ node, messageId: 'literal', data: { text: text.trim().slice(0, 40) } });

    return {
      JSXText(node) {
        if (hasText(node.value)) report(node, node.value);
      },
      JSXExpressionContainer(node) {
        if (node.parent.type !== 'JSXElement' && node.parent.type !== 'JSXFragment') return;
        const text = literalText(node.expression);
        if (text) report(node, text);
      },
      JSXAttribute(node) {
        const name = propName(node);
        if (!name || (!USER_FACING.has(name) && name !== 'children')) return;
        const value =
          node.value?.type === 'JSXExpressionContainer' ? node.value.expression : node.value;
        const text = literalText(value);
        if (text) report(node, text);
      },
      Property(node) {
        const name = propName(node);
        if (!name || !USER_FACING.has(name)) return;
        const text = literalText(node.value);
        if (text) report(node, text);
      },
    };
  },
};

/** Physical-direction style keys and their logical (RTL-safe) replacements. */
const PHYSICAL = {
  marginLeft: 'marginStart',
  marginRight: 'marginEnd',
  paddingLeft: 'paddingStart',
  paddingRight: 'paddingEnd',
  left: 'start',
  right: 'end',
  borderLeftWidth: 'borderStartWidth',
  borderRightWidth: 'borderEndWidth',
  borderLeftColor: 'borderStartColor',
  borderRightColor: 'borderEndColor',
  borderTopLeftRadius: 'borderTopStartRadius',
  borderTopRightRadius: 'borderTopEndRadius',
  borderBottomLeftRadius: 'borderBottomStartRadius',
  borderBottomRightRadius: 'borderBottomEndRadius',
};

const rtlSafeStyles = {
  meta: {
    type: 'problem',
    docs: { description: 'Use logical start/end style properties so layouts mirror in RTL.' },
    messages: {
      physical: 'Use "{{logical}}" instead of "{{physical}}" so the layout mirrors in RTL.',
      align:
        'Use textAlign "auto" (or omit it) instead of "{{value}}" so text follows the language.',
    },
    schema: [],
  },
  create(context) {
    return {
      Property(node) {
        const name = propName(node);
        if (!name) return;
        if (Object.hasOwn(PHYSICAL, name)) {
          context.report({
            node: node.key,
            messageId: 'physical',
            data: { physical: name, logical: PHYSICAL[name] },
          });
        } else if (
          name === 'textAlign' &&
          node.value.type === 'Literal' &&
          (node.value.value === 'left' || node.value.value === 'right')
        ) {
          context.report({
            node: node.value,
            messageId: 'align',
            data: { value: node.value.value },
          });
        }
      },
    };
  },
};

module.exports = {
  meta: { name: 'card-sharing-local' },
  rules: {
    'no-hardcoded-strings': noHardcodedStrings,
    'rtl-safe-styles': rtlSafeStyles,
  },
};
