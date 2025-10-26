import { createHighlighter } from 'shiki';
import miseTomlGrammar from './.vitepress/grammars/mise-toml.tmLanguage.json' with { type: 'json' };
import kdlGrammar from './.vitepress/grammars/kdl.tmLanguage.json' with { type: 'json' };

const code = `[tasks.deploy]
description = "Deploy application"
usage = '''
arg "<environment>" help="Target environment"
flag "-v --verbose" help="Enable verbose output"
'''
run = '''
echo "Deploying to \${usage_environment?}"
./deploy.sh "\${usage_environment?}"
'''`;

try {
  const highlighter = await createHighlighter({
    themes: ['github-dark'],
    langs: [
      'shell',
      'bash',
      'toml',
      {
        ...kdlGrammar,
        name: 'kdl',
        scopeName: 'source.kdl',
      },
      {
        ...miseTomlGrammar,
        name: 'mise-toml',
        aliases: ['mise.toml'],
        scopeName: 'source.mise-toml',
      }
    ]
  });

  const html = highlighter.codeToHtml(code, {
    lang: 'mise-toml',
    theme: 'github-dark'
  });

  console.log('HTML output:', html);
  console.log('\n\nSuccess! Grammar is working.');
} catch (error) {
  console.error('Error:', error.message);
  console.error(error);
}
