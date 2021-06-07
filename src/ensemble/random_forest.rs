//! Random forests.
//!
//! Fits a forest of decision trees using bootstrap samples of
//! training data. The predictions of individual trees are averaged
//! to arrive at the final prediction. This counters the tendency
//! of individual trees to overfit and provides better out-of-sample
//! predictions. In general, the more trees fit, the higher the accuracy.
//!
//! # Examples
//!
//! ```
//! use rustlearn::prelude::*;
//!
//! use rustlearn::ensemble::random_forest::Hyperparameters;
//! use rustlearn::datasets::iris;
//! use rustlearn::trees::decision_tree;
//!
//! let (data, target) = iris::load_data();
//!
//! let mut tree_params = decision_tree::Hyperparameters::new(data.cols());
//! tree_params.min_samples_split(10)
//!     .max_features(4);
//!
//! let mut model = Hyperparameters::new(tree_params, 10)
//!     .one_vs_rest();
//!
//! model.fit(&data, &target).unwrap();
//!
//! let prediction = model.predict(&data).unwrap();
//! ```

use std::usize;

use crate::prelude::*;

use crate::trees::decision_tree;

use crate::multiclass::OneVsRestWrapper;
use crate::utils::EncodableRng;

use rand::prelude::*;
use rand::distributions::Uniform;

#[derive(Serialize, Deserialize)]
pub struct Hyperparameters {
    tree_hyperparameters: decision_tree::Hyperparameters,
    num_trees: usize,
    rng: EncodableRng,
}

impl Hyperparameters {
    /// Create a new instance of Hyperparameters, using the Hyperparameters
    /// for a `DecisionTree` and the number of trees to build.
    pub fn new(
        tree_hyperparameters: decision_tree::Hyperparameters,
        num_trees: usize,
    ) -> Hyperparameters {
        Hyperparameters {
            tree_hyperparameters: tree_hyperparameters,
            num_trees: num_trees,
            rng: EncodableRng::new(),
        }
    }

    /// Set the random number generator.
    pub fn rng(&mut self, rng: StdRng) -> &mut Hyperparameters {
        self.rng.rng = rng;
        self
    }

    /// Build the random forest model.
    pub fn build(&self) -> RandomForest {
        let mut trees = Vec::with_capacity(self.num_trees);

        let mut rng = self.rng.clone();

        for _ in 0..self.num_trees {
            // Reseed trees to introduce randomness,
            // without this they are just copies of each other
            let range = Uniform::new(0, u8::MAX);

            let mut hyperparams = self.tree_hyperparameters.clone();
            let mut seed = [0; 32];
            for s in seed.iter_mut() {
                *s = rng.rng.sample(range);
            }
            hyperparams.rng(SeedableRng::from_seed(seed));

            trees.push(hyperparams.build());
        }

        RandomForest {
            trees: trees,
            rng: self.rng.clone(),
        }
    }

    /// Build a one-vs-rest multiclass random forest.
    pub fn one_vs_rest(&mut self) -> OneVsRestWrapper<RandomForest> {
        let base_model = self.build();

        OneVsRestWrapper::new(base_model)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RandomForest {
    trees: Vec<decision_tree::DecisionTree>,
    rng: EncodableRng,
}

impl<'a> SupervisedModel<&'a Array> for RandomForest {
    fn fit(&mut self, X: &Array, y: &Array) -> Result<(), &'static str> {
        let mut rng = self.rng.clone();

        for tree in &mut self.trees {
            let indices = RandomForest::bootstrap_indices(X.rows(), &mut rng.rng);
            tree.fit(&X.get_rows(&indices), &y.get_rows(&indices))?;
        }

        self.rng = rng;

        Ok(())
    }

    fn decision_function(&self, X: &Array) -> Result<Array, &'static str> {
        let mut df = Array::zeros(X.rows(), 1);

        for tree in &self.trees {
            df.add_inplace(&tree.decision_function(X)?);
        }

        df.div_inplace(self.trees.len() as f32);

        Ok(df)
    }
}

impl<'a> SupervisedModel<&'a SparseRowArray> for RandomForest {
    fn fit(&mut self, X: &SparseRowArray, y: &Array) -> Result<(), &'static str> {
        let mut rng = self.rng.clone();

        for tree in &mut self.trees {
            let indices = RandomForest::bootstrap_indices(X.rows(), &mut rng.rng);
            let x = SparseColumnArray::from(&X.get_rows(&indices));
            tree.fit(&x, &y.get_rows(&indices))?;
        }

        self.rng = rng;

        Ok(())
    }

    fn decision_function(&self, X: &SparseRowArray) -> Result<Array, &'static str> {
        let mut df = Array::zeros(X.rows(), 1);

        let x = SparseColumnArray::from(X);

        for tree in &self.trees {
            df.add_inplace(&tree.decision_function(&x)?);
        }

        df.div_inplace(self.trees.len() as f32);

        Ok(df)
    }
}

impl RandomForest {
    /// Return a reference to the consituent trees vector.
    pub fn trees(&self) -> &Vec<decision_tree::DecisionTree> {
        &self.trees
    }

    fn bootstrap_indices(num_indices: usize, rng: &mut StdRng) -> Vec<usize> {
        let range = Uniform::new(0, num_indices - 1);

        (0..num_indices)
            .map(|_| rng.sample(range))
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use crate::trees::decision_tree;

    use super::*;
    use crate::cross_validation::cross_validation::CrossValidation;
    use crate::datasets::iris::load_data;
    use crate::metrics::accuracy_score;
    use crate::multiclass::OneVsRestWrapper;
    use crate::utils::std_rng;

    use bincode;
    use serde_json;

    #[cfg(feature = "all_tests")]
    use crate::datasets::newsgroups;

    #[test]
    fn test_random_forest_iris() {
        let (data, target) = load_data();

        let mut test_accuracy = 0.0;

        let no_splits = 10;

        let mut cv = CrossValidation::new(data.rows(), no_splits);
        cv.set_rng(std_rng());

        for (train_idx, test_idx) in cv {
            let x_train = data.get_rows(&train_idx);
            let x_test = data.get_rows(&test_idx);

            let y_train = target.get_rows(&train_idx);

            let mut tree_params = decision_tree::Hyperparameters::new(data.cols());
            tree_params
                .min_samples_split(10)
                .max_features(4)
                .rng(std_rng());

            let mut model = Hyperparameters::new(tree_params, 10)
                .rng(std_rng())
                .one_vs_rest();

            model.fit(&x_train, &y_train).unwrap();

            let test_prediction = model.predict(&x_test).unwrap();

            test_accuracy += accuracy_score(&target.get_rows(&test_idx), &test_prediction);
        }

        test_accuracy /= no_splits as f32;

        println!("Accuracy {}", test_accuracy);

        assert!(test_accuracy > 0.96);
    }

    #[test]
    fn test_random_forest_iris_parallel() {
        let (data, target) = load_data();

        let mut test_accuracy = 0.0;

        let no_splits = 10;

        let mut cv = CrossValidation::new(data.rows(), no_splits);
        cv.set_rng(std_rng());

        for (train_idx, test_idx) in cv {
            let x_train = data.get_rows(&train_idx);
            let x_test = data.get_rows(&test_idx);

            let y_train = target.get_rows(&train_idx);

            let mut tree_params = decision_tree::Hyperparameters::new(data.cols());
            tree_params
                .min_samples_split(10)
                .max_features(4)
                .rng(std_rng());

            let mut model = Hyperparameters::new(tree_params, 10)
                .rng(std_rng())
                .one_vs_rest();

            model.fit_parallel(&x_train, &y_train, 2).unwrap();

            let test_prediction = model.predict_parallel(&x_test, 2).unwrap();

            test_accuracy += accuracy_score(&target.get_rows(&test_idx), &test_prediction);
        }

        test_accuracy /= no_splits as f32;

        println!("Accuracy {}", test_accuracy);

        assert!(test_accuracy > 0.96);
    }

    #[test]
    fn test_random_forest_iris_sparse() {
        let (data, target) = load_data();

        let mut test_accuracy = 0.0;

        let no_splits = 10;

        let mut cv = CrossValidation::new(data.rows(), no_splits);
        cv.set_rng(std_rng());

        for (train_idx, test_idx) in cv {
            let x_train = SparseRowArray::from(&data.get_rows(&train_idx));
            let x_test = SparseRowArray::from(&data.get_rows(&test_idx));

            let y_train = target.get_rows(&train_idx);

            let mut tree_params = decision_tree::Hyperparameters::new(data.cols());
            tree_params
                .min_samples_split(10)
                .max_features(4)
                .rng(std_rng());

            let mut model = Hyperparameters::new(tree_params, 10)
                .rng(std_rng())
                .one_vs_rest();

            model.fit(&x_train, &y_train).unwrap();

            let test_prediction = model.predict(&x_test).unwrap();

            test_accuracy += accuracy_score(&target.get_rows(&test_idx), &test_prediction);
        }

        test_accuracy /= no_splits as f32;

        println!("Accuracy {}", test_accuracy);

        assert!(test_accuracy > 0.96);
    }

    #[test]
    #[cfg(feature = "all_tests")]
    fn test_random_forest_newsgroups() {
        extern crate time;

        let (X, target) = newsgroups::load_data();

        let no_splits = 2;

        let mut test_accuracy = 0.0;
        let mut train_accuracy = 0.0;

        let mut cv = CrossValidation::new(X.rows(), no_splits);
        cv.set_rng(std_rng());

        for (train_idx, test_idx) in cv {
            let x_train = X.get_rows(&train_idx);
            let x_test = X.get_rows(&test_idx);

            let y_train = target.get_rows(&train_idx);

            let mut tree_params = decision_tree::Hyperparameters::new(X.cols());
            tree_params
                .min_samples_split(5)
                .rng(std_rng());

            let mut model = Hyperparameters::new(tree_params, 20).one_vs_rest();

            let start = time::precise_time_ns();

            model.fit(&x_train, &y_train).unwrap();
            println!("Elapsed {}", time::precise_time_ns() - start);

            let y_hat = model.predict(&x_test).unwrap();
            let y_hat_train = model.predict(&x_train).unwrap();

            test_accuracy += accuracy_score(&target.get_rows(&test_idx), &y_hat);

            train_accuracy += accuracy_score(&target.get_rows(&train_idx), &y_hat_train);
        }

        test_accuracy /= no_splits as f32;
        train_accuracy /= no_splits as f32;
        println!("{}", test_accuracy);
        println!("train accuracy {}", train_accuracy);

        assert!(train_accuracy > 0.95);
    }

    #[test]
    fn serialization() {
        let (data, target) = load_data();

        let mut test_accuracy = 0.0;

        let no_splits = 10;

        let mut cv = CrossValidation::new(data.rows(), no_splits);
        cv.set_rng(std_rng());

        for (train_idx, test_idx) in cv {
            let x_train = data.get_rows(&train_idx);
            let x_test = data.get_rows(&test_idx);

            let y_train = target.get_rows(&train_idx);

            let mut tree_params = decision_tree::Hyperparameters::new(data.cols());
            tree_params
                .min_samples_split(10)
                .max_features(4)
                .rng(std_rng());

            let mut model = Hyperparameters::new(tree_params, 10)
                .rng(std_rng())
                .one_vs_rest();

            model.fit(&x_train, &y_train).unwrap();

            let encoded = bincode::serialize(&model).unwrap();
            let decoded: OneVsRestWrapper<RandomForest> = bincode::deserialize(&encoded).unwrap();

            let bincode_prediction = decoded.predict(&x_test).unwrap();

            // JSON encoding
            let encoded = serde_json::to_string(&model).unwrap();
            let decoded: OneVsRestWrapper<RandomForest> = serde_json::from_str(&encoded).unwrap();

            let json_prediction = decoded.predict(&x_test).unwrap();

            assert!(allclose(&json_prediction, &bincode_prediction));

            test_accuracy += accuracy_score(&target.get_rows(&test_idx), &json_prediction);
        }

        test_accuracy /= no_splits as f32;

        println!("Accuracy {}", test_accuracy);

        assert!(test_accuracy > 0.96);
    }
}
