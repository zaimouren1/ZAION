const input = JSON.parse(process.argv[2] || '{}');
const { a = 0, b = 0, op = 'add' } = input;
const ops = { add: a+b, sub: a-b, mul: a*b, div: b !== 0 ? a/b : null };
console.log(JSON.stringify({ result: ops[op], op }));
