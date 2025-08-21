// A configuration file of lintnet.
// https://lintnet.github.io/
function(param) {
  targets: [
    {
      data_files: [
        '**/*',
      ],
      modules: [
        {
          path: 'github_archive/github.com/lintnet-modules/nllint/main.jsonnet@a36d23d28936a85df8cad6e831c16854e9e2caa6:v0.2.0',
          config: {
            trim_space: true,
          },
        },
      ],
    },
    {
      data_files: [
        '.github/workflows/*.yml',
        '.github/workflows/*.yaml',
      ],
      modules: [
        'github_archive/github.com/lintnet-modules/ghalint/workflow/**/main.jsonnet@c311ef7a7e3acdfb8a65136b7852e0619be84c1d:v0.3.3',
      ],
    },
  ],
}
