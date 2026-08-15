// Debug skill: prints all argv to stdout as JSON
console.log(JSON.stringify({
  argc: process.argv.length,
  argv0: process.argv[0],
  argv1: process.argv[1],
  argv2: process.argv[2],
  argv3: process.argv[3],
  all: process.argv
}));
