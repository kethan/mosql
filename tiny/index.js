import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';

const tiny = createQueryBuilder({
  filterOps: {
    $eq: filterOps.$eq,
    $ne: filterOps.$ne,
    $gt: filterOps.$gt,
    $gte: filterOps.$gte,
    $lt: filterOps.$lt,
    $lte: filterOps.$lte,
    $in: filterOps.$in,
    $nin: filterOps.$nin,
    $like: filterOps.$like,
    $exists: filterOps.$exists,
  },
  exprOps: {
    $add: exprOps.$add,
    $subtract: exprOps.$subtract,
    $multiply: exprOps.$multiply,
    $divide: exprOps.$divide,
    $concat: exprOps.$concat,
    $upper: exprOps.$upper,
    $lower: exprOps.$lower,
    $eq: exprOps.$eq,
    $cond: exprOps.$cond,
    $sum: exprOps.$sum,
    $avg: exprOps.$avg,
    $min: exprOps.$min,
    $max: exprOps.$max,
  },
  updateOps: {
    $set: updateOps.$set,
  },
  stageHandlers: {
    $match: stageHandlers.$match,
    $project: stageHandlers.$project,
    $group: stageHandlers.$group,
    $sort: stageHandlers.$sort,
    $limit: stageHandlers.$limit,
    $skip: stageHandlers.$skip,
    $count: stageHandlers.$count,
  }
});

export const { collection, aggregate, filter, db } = tiny;
export default tiny;