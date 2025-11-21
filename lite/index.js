import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers as fullStages } from '../src/index.js';

const allowedExpr = [
  '$add', '$subtract', '$multiply', '$divide', '$mod', '$abs', '$ceil', '$floor', '$round', '$pow', '$sqrt',
  '$concat', '$upper', '$lower', '$substr', '$trim', '$ltrim', '$rtrim', '$strLen', '$replace',
  '$eq', '$ne', '$gt', '$gte', '$lt', '$lte', '$in', '$nin',
  '$and', '$or', '$not', '$cond', '$ifNull', '$exists',
  '$toString', '$toInt', '$toDouble',
  '$sum', '$avg', '$min', '$max', '$count'
];

const liteExprOps = Object.fromEntries(Object.entries(exprOps).filter(([k]) => allowedExpr.includes(k)));

const basicStages = (({ $match, $project, $group, $sort, $limit, $skip, $count }) => ({ $match, $project, $group, $sort, $limit, $skip, $count }))(fullStages);

const lite = createQueryBuilder({ filterOps, exprOps: liteExprOps, updateOps, stageHandlers: basicStages });

export default lite;
export const { collection, filter, expression, aggregate, extend, db } = lite;
 
