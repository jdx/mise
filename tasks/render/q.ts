import path = require('node:path');
import completion from '../../completions/mise'
import fsAsync = require('node:fs/promises');


const main = async () => {
  let newSpec = (completion as any);
  newSpec.subcommands.find((v) => v.name[0] === 'run').args.find(v => v.name === 'task').generators = "$GENERATOR_REPLACE$"
  const content = `
const taskGenerator: Fig.Generator = {
  script: ["sh", "-c", "mise tasks -J"],

  postProcess: (out) => {
    return JSON.parse(out).map(v => ({ name:v.name, description: v.description, icon: "fig://icon?type=command"} as Fig.Suggestion)) 
  }
}
const completion: Fig.Spec = ${JSON.stringify(newSpec, null, 2).replace(`"$GENERATOR_REPLACE$"`, 'taskGenerator')}
export default completion;`

  await fsAsync.writeFile('./completions/mise.ts', content);

}


main();
