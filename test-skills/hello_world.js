const input = JSON.parse(process.argv[2] || '{}');
const name = input.name || 'World';
console.log(JSON.stringify({ greeting: `Hello, ${name}! (node)` }));
